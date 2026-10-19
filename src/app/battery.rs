//! Read-only internal-battery telemetry. One notification/cache worker belongs to the shared owner.

use std::ffi::c_void;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFAllocatorRef, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::runloop::{
  CFRunLoop, CFRunLoopRunResult, CFRunLoopSource, CFRunLoopSourceRef, kCFRunLoopDefaultMode,
};
use core_foundation::string::CFString;
use serde::{Deserialize, Serialize};

const REFRESH: Duration = Duration::from_secs(30);
pub(crate) const STALE_MS: u64 = 60_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Snapshot {
  pub state: State,
  /// Acquisition ages at completion of the containing system sample, not hardware polling ages.
  pub age_ms: Option<u64>,
  pub registry_age_ms: Option<u64>,
  pub source: PowerSource,
  pub percent: Option<f64>,
  pub charging: Option<bool>,
  pub health: Option<Health>,
  pub capacity_health_percent: Option<f64>,
  pub cycles: Option<u32>,
  pub temperature_celsius: Option<f64>,
  pub voltage_v: Option<f64>,
  pub current_a: Option<f64>,
  /// Positive is charging, negative is discharging. Battery electrical flow, never system/chip
  /// power.
  pub electrical_power_w: Option<f64>,
  pub time_to_empty_min: Option<u32>,
  pub time_to_full_min: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
  #[default]
  Unavailable,
  Absent,
  Available,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PowerSource {
  Ac,
  Battery,
  Ups,
  Offline,
  #[default]
  Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Health {
  Good,
  Fair,
  Poor,
}

#[derive(Default)]
struct Registry {
  cycles: Option<u32>,
  voltage_v: Option<f64>,
  current_a: Option<f64>,
  charging: Option<bool>,
  external: Option<bool>,
}
#[derive(Default)]
struct Cached {
  snapshot: Snapshot,
  acquired: Option<Instant>,
  registry_acquired: Option<Instant>,
}
#[derive(Clone, Default)]
pub(crate) struct Reader(Arc<Mutex<Cached>>);
impl Reader {
  pub(crate) fn snapshot(&self) -> Snapshot {
    let cached = self.0.lock().unwrap();
    let mut snapshot = cached.snapshot.clone();
    let age = |time: Instant| time.elapsed().as_millis().min(u64::MAX as u128) as u64;
    snapshot.age_ms = cached.acquired.map(age);
    snapshot.registry_age_ms = cached.registry_acquired.map(age);
    snapshot
  }
}

/// The guard is held by source::run, including its error paths. Drop joins the only battery worker.
pub(crate) struct Worker {
  pub reader: Reader,
  stopped: Arc<AtomicBool>,
  worker: Option<JoinHandle<()>>,
}
impl Worker {
  pub(crate) fn start() -> std::io::Result<Self> {
    let reader = Reader(Arc::new(Mutex::new(Cached::default())));
    let cache = reader.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&stopped);
    let worker = thread::Builder::new().name("macmon-battery".into()).spawn(move || {
      run(cache, stop);
    })?;
    Ok(Self { reader, stopped, worker: Some(worker) })
  }
}
impl Drop for Worker {
  fn drop(&mut self) {
    self.stopped.store(true, Ordering::Release);
    if let Some(worker) = self.worker.take() {
      worker.thread().unpark();
      let _ = worker.join();
    }
  }
}

unsafe extern "C" fn changed(context: *mut c_void) {
  // Callback and context live on the same worker; it is invalidated before the Box is dropped.
  unsafe { &*(context.cast::<AtomicBool>()) }.store(true, Ordering::Release);
}
fn run(cache: Reader, stopped: Arc<AtomicBool>) {
  let dirty = Box::new(AtomicBool::new(true));
  let raw = unsafe {
    IOPSNotificationCreateRunLoopSource(changed, (&*dirty as *const AtomicBool).cast_mut().cast())
  };
  let notification =
    (!raw.is_null()).then(|| unsafe { CFRunLoopSource::wrap_under_create_rule(raw) });
  let loop_ = CFRunLoop::get_current();
  if let Some(source) = &notification {
    loop_.add_source(source, unsafe { kCFRunLoopDefaultMode });
  }
  let mut last = None::<Instant>;
  let mut registry = Registry::default();
  let mut registry_acquired = None::<Instant>;
  let mut previous = None;
  let mut registry_attempted = None::<Instant>;
  while !stopped.load(Ordering::Acquire) {
    let now = Instant::now();
    if due(last, dirty.load(Ordering::Acquire), now) {
      dirty.store(false, Ordering::Release);
      let mut snapshot = read_sources();
      let acquired = Instant::now();
      if snapshot.state == State::Available {
        let state = (snapshot.source, snapshot.charging);
        if previous != Some(state)
          || registry_attempted.is_none_or(|then| now.duration_since(then) >= REFRESH)
        {
          registry_attempted = Some(now);
          let read = read_registry();
          registry_acquired = read.as_ref().map(|_| Instant::now());
          registry = read.unwrap_or_default();
        }
        previous = Some(state);
        enrich(&mut snapshot, &registry);
      } else {
        registry = Registry::default();
        registry_acquired = None;
        registry_attempted = None;
        previous = None;
      }
      let mut cached = cache.0.lock().unwrap();
      cached.snapshot = snapshot;
      cached.acquired = Some(acquired);
      cached.registry_acquired = registry_acquired;
      last = Some(Instant::now());
    }
    if notification.is_some() {
      let result =
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, Duration::from_millis(250), false);
      if matches!(result, CFRunLoopRunResult::Finished | CFRunLoopRunResult::Stopped) {
        thread::park_timeout(Duration::from_millis(250));
      }
    } else {
      thread::park_timeout(Duration::from_millis(250));
    }
  }
  if let Some(source) = notification {
    loop_.remove_source(&source, unsafe { kCFRunLoopDefaultMode });
    unsafe { core_foundation::runloop::CFRunLoopSourceInvalidate(source.as_concrete_TypeRef()) };
  }
}
fn enrich(snapshot: &mut Snapshot, registry: &Registry) {
  snapshot.cycles = registry.cycles;
  // Registry voltage/current are acquired together, never mixed with another source's values.
  let pair = if snapshot.voltage_v.is_some() && snapshot.current_a.is_some() {
    (snapshot.voltage_v, snapshot.current_a)
  } else {
    (
      registry.voltage_v,
      registry.current_a.filter(|_| {
        registry.charging == snapshot.charging
          && match snapshot.source {
            PowerSource::Ac => registry.external == Some(true),
            PowerSource::Battery => registry.external == Some(false),
            _ => false,
          }
      }),
    )
  };
  snapshot.voltage_v = pair.0;
  snapshot.current_a = coherent_current(pair.1, snapshot.charging, snapshot.source);
  snapshot.electrical_power_w = snapshot.voltage_v.zip(snapshot.current_a).map(|(v, a)| v * a);
}
fn due(last: Option<Instant>, dirty: bool, now: Instant) -> bool {
  last.is_none_or(|last| {
    let elapsed = now.saturating_duration_since(last);
    elapsed >= REFRESH || (dirty && elapsed >= Duration::from_secs(1))
  })
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
  fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
  fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
  fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFDictionaryRef;
  fn IOPSGetProvidingPowerSourceType(blob: CFTypeRef) -> core_foundation::string::CFStringRef;
  fn IOPSNotificationCreateRunLoopSource(
    callback: unsafe extern "C" fn(*mut c_void),
    context: *mut c_void,
  ) -> CFRunLoopSourceRef;
  fn IOServiceMatching(name: *const i8) -> CFDictionaryRef;
  fn IOServiceGetMatchingService(port: u32, matching: CFDictionaryRef) -> u32;
  fn IORegistryEntryCreateCFProperties(
    entry: u32,
    properties: *mut CFMutableDictionaryRef,
    allocator: CFAllocatorRef,
    options: u32,
  ) -> i32;
  fn IOObjectRelease(object: u32) -> u32;
}

type Dictionary = CFDictionary<CFString, CFType>;
fn value(dict: &Dictionary, key: &str) -> Option<CFType> {
  dict.find(CFString::new(key)).map(|value| (*value).clone())
}
fn num(dict: &Dictionary, key: &str) -> Option<i64> {
  value(dict, key)?.downcast::<CFNumber>()?.to_i64()
}
fn boolean(dict: &Dictionary, key: &str) -> Option<bool> {
  value(dict, key)?.downcast::<CFBoolean>().map(|value| value.into())
}
fn string(dict: &Dictionary, key: &str) -> Option<String> {
  value(dict, key)?.downcast::<CFString>().map(|value| value.to_string())
}
fn source(value: &str) -> PowerSource {
  match value {
    "AC Power" => PowerSource::Ac,
    "Battery Power" => PowerSource::Battery,
    "UPS Power" => PowerSource::Ups,
    "Off Line" => PowerSource::Offline,
    _ => PowerSource::Unknown,
  }
}
fn percentage(current: Option<i64>, maximum: Option<i64>) -> Option<f64> {
  current
    .zip(maximum)
    .filter(|(n, d)| *d > 0 && *n >= 0 && n <= d)
    .map(|(n, d)| n as f64 / d as f64 * 100.0)
}
fn minutes(value: Option<i64>) -> Option<u32> {
  value.filter(|value| (0..65_535).contains(value)).map(|value| value as u32)
}
fn millivolts(value: Option<i64>) -> Option<f64> {
  value.filter(|value| (1_000..=50_000).contains(value)).map(|value| value as f64 / 1000.0)
}
fn milliamps(value: Option<i64>) -> Option<f64> {
  let value = value?;
  // IOPMPowerSource exports a signed 32-bit current; CFNumber may expose its unsigned bit pattern.
  let value = if (i32::MAX as i64..=u32::MAX as i64).contains(&value) {
    value as u32 as i32 as i64
  } else {
    value
  };
  (-50_000..=50_000).contains(&value).then_some(value as f64 / 1000.0)
}
fn coherent_current(
  current: Option<f64>,
  charging: Option<bool>,
  source: PowerSource,
) -> Option<f64> {
  current.filter(|current| {
    !((charging == Some(true) && (*current < 0.0 || source == PowerSource::Battery))
      || (charging == Some(false) && *current > 0.0))
  })
}
fn parse(dict: &Dictionary, providing: PowerSource) -> Snapshot {
  let power_source = string(dict, "Power Source State").map_or(providing, |value| source(&value));
  let charging = boolean(dict, "Is Charging");
  let health = match string(dict, "BatteryHealth").as_deref() {
    Some("Good") => Some(Health::Good),
    Some("Fair") => Some(Health::Fair),
    Some("Poor") => Some(Health::Poor),
    _ => None,
  };
  Snapshot {
    state: State::Available,
    source: power_source,
    percent: percentage(num(dict, "Current Capacity"), num(dict, "Max Capacity")),
    charging,
    health,
    capacity_health_percent: percentage(num(dict, "Nominal Capacity"), num(dict, "DesignCapacity")),
    // Public IOPS temperature is Celsius. IORegistry's differently scaled Temperature is not used.
    temperature_celsius: num(dict, "Temperature")
      .filter(|value| (-40..=100).contains(value))
      .map(|v| v as f64),
    voltage_v: millivolts(num(dict, "Voltage")),
    current_a: milliamps(num(dict, "Current")),
    time_to_empty_min: (power_source == PowerSource::Battery && charging == Some(false))
      .then(|| minutes(num(dict, "Time to Empty")))
      .flatten(),
    time_to_full_min: (power_source == PowerSource::Ac && charging == Some(true))
      .then(|| minutes(num(dict, "Time to Full Charge")))
      .flatten(),
    ..Snapshot::default()
  }
}
fn read_sources() -> Snapshot {
  let raw = unsafe { IOPSCopyPowerSourcesInfo() };
  if raw.is_null() {
    return Snapshot::default();
  }
  let blob = unsafe { CFType::wrap_under_create_rule(raw) };
  let raw = unsafe { IOPSCopyPowerSourcesList(blob.as_CFTypeRef()) };
  if raw.is_null() {
    return Snapshot::default();
  }
  let list = unsafe { CFArray::<CFType>::wrap_under_create_rule(raw) };
  let providing = unsafe { IOPSGetProvidingPowerSourceType(blob.as_CFTypeRef()) };
  let providing = if providing.is_null() {
    PowerSource::Unknown
  } else {
    source(&unsafe { CFString::wrap_under_get_rule(providing) }.to_string())
  };
  let mut internal = Vec::new();
  for handle in list.iter() {
    let raw = unsafe { IOPSGetPowerSourceDescription(blob.as_CFTypeRef(), handle.as_CFTypeRef()) };
    if raw.is_null() {
      return Snapshot::default();
    }
    let dict = unsafe { Dictionary::wrap_under_get_rule(raw) };
    match description(&dict, providing) {
      Ok(Some(snapshot)) => internal.push(snapshot),
      Ok(None) => {}
      Err(()) => return Snapshot::default(),
    }
  }
  select(internal, providing)
}
fn description(dict: &Dictionary, providing: PowerSource) -> Result<Option<Snapshot>, ()> {
  // IOPSKeys.h requires Type and defines only InternalBattery/UPS for this source list.
  // A broken or unfamiliar description does not establish that an internal battery is absent.
  match string(dict, "Type").as_deref() {
    Some("InternalBattery") => match boolean(dict, "Is Present") {
      Some(true) => Ok(Some(parse(dict, providing))),
      Some(false) => Ok(None),
      None => Err(()),
    },
    Some("UPS") => Ok(None),
    _ => Err(()),
  }
}
fn select(mut internal: Vec<Snapshot>, providing: PowerSource) -> Snapshot {
  match internal.len() {
    0 => Snapshot { state: State::Absent, source: providing, ..Snapshot::default() },
    1 => internal.pop().unwrap(),
    _ => Snapshot::default(), // Do not mix separate battery packs or guess their combined capacity.
  }
}
fn read_registry() -> Option<Registry> {
  let matching = unsafe { IOServiceMatching(c"AppleSmartBattery".as_ptr()) };
  if matching.is_null() {
    return None;
  }
  let entry = unsafe { IOServiceGetMatchingService(0, matching) };
  if entry == 0 {
    return None;
  }
  let mut raw = std::ptr::null_mut();
  let status = unsafe { IORegistryEntryCreateCFProperties(entry, &mut raw, null(), 0) };
  unsafe { IOObjectRelease(entry) };
  if raw.is_null() {
    return None;
  }
  let dict = unsafe { Dictionary::wrap_under_create_rule(raw) };
  if status != 0 {
    return None;
  }
  if boolean(&dict, "BatteryInstalled") != Some(true) {
    return None;
  }
  Some(Registry {
    cycles: num(&dict, "CycleCount")
      .and_then(|value| u32::try_from(value).ok())
      .filter(|v| *v < 1_000_000),
    voltage_v: millivolts(num(&dict, "Voltage")),
    current_a: milliamps(num(&dict, "Amperage")),
    charging: boolean(&dict, "IsCharging"),
    external: boolean(&dict, "ExternalConnected"),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  fn dict(values: &[(&str, CFType)]) -> Dictionary {
    Dictionary::from_CFType_pairs(
      &values.iter().map(|(key, value)| (CFString::new(key), value.clone())).collect::<Vec<_>>(),
    )
  }
  fn n(value: i64) -> CFType {
    CFNumber::from(value).as_CFType()
  }
  fn s(value: &str) -> CFType {
    CFString::new(value).as_CFType()
  }
  fn b(value: bool) -> CFType {
    CFBoolean::from(value).as_CFType()
  }

  #[test]
  fn ac_is_not_necessarily_charging_and_health_is_not_normalized_max_capacity() {
    let sample = parse(
      &dict(&[
        ("Power Source State", s("AC Power")),
        ("Current Capacity", n(81)),
        ("Max Capacity", n(100)),
        ("Is Charging", b(false)),
        ("BatteryHealth", s("Good")),
        ("Time to Empty", n(0)),
        ("Time to Full Charge", n(0)),
        ("DesignCapacity", n(6075)),
      ]),
      PowerSource::Unknown,
    );
    assert_eq!(sample.percent, Some(81.0));
    assert_eq!(sample.source, PowerSource::Ac);
    assert_eq!(sample.charging, Some(false));
    assert_eq!(sample.health, Some(Health::Good));
    assert_eq!(sample.capacity_health_percent, None);
    assert_eq!(sample.time_to_empty_min, None);
    assert_eq!(sample.time_to_full_min, None);
    assert_eq!(sample.electrical_power_w, None);
  }
  #[test]
  fn charging_discharge_time_and_documented_units_are_contextual() {
    let mut values = vec![
      ("Power Source State", s("Battery Power")),
      ("Current Capacity", n(0)),
      ("Max Capacity", n(100)),
      ("Is Charging", b(false)),
      ("Time to Empty", n(0)),
      ("Time to Full Charge", n(500)),
      ("Voltage", n(12000)),
      ("Current", n(4294965296)),
      ("Temperature", n(30)),
    ];
    let sample = parse(&dict(&values), PowerSource::Unknown);
    assert_eq!(sample.percent, Some(0.0));
    assert_eq!(sample.voltage_v, Some(12.0));
    assert_eq!(sample.current_a, Some(-2.0));
    assert_eq!(sample.temperature_celsius, Some(30.0));
    assert_eq!(sample.time_to_empty_min, Some(0));
    assert_eq!(sample.time_to_full_min, None);
    values[0] = ("Power Source State", s("AC Power"));
    values[3] = ("Is Charging", b(true));
    let sample = parse(&dict(&values), PowerSource::Unknown);
    assert_eq!(sample.time_to_full_min, Some(500));
    assert_eq!(sample.time_to_empty_min, None);
    assert_eq!(coherent_current(sample.current_a, sample.charging, sample.source), None);
    assert_eq!(coherent_current(Some(2.0), Some(true), PowerSource::Ac), Some(2.0));
    assert_eq!(coherent_current(Some(0.0), Some(false), PowerSource::Ac), Some(0.0));
    assert_eq!(coherent_current(Some(2.0), Some(false), PowerSource::Battery), None);
    assert_eq!(coherent_current(Some(-2.0), Some(true), PowerSource::Battery), None);
  }
  #[test]
  fn unknown_malformed_and_incoherent_values_stay_unavailable() {
    let sample = parse(
      &dict(&[
        ("Current Capacity", n(101)),
        ("Max Capacity", n(100)),
        ("Is Charging", n(1)),
        ("Voltage", n(0)),
        ("Current", n(65535)),
        ("Temperature", n(3023)),
        ("BatteryHealth", s("Unknown")),
      ]),
      PowerSource::Ac,
    );
    assert_eq!(sample.percent, None);
    assert_eq!(sample.charging, None);
    assert_eq!(sample.voltage_v, None);
    assert_eq!(sample.current_a, None);
    assert_eq!(sample.temperature_celsius, None);
    assert_eq!(sample.health, None);
    assert_eq!(percentage(Some(100), Some(0)), None);
    assert_eq!(percentage(Some(-1), Some(100)), None);
    for value in [-1, -2, 65535, i64::MAX] {
      assert_eq!(minutes(Some(value)), None);
    }
    assert_eq!(milliamps(Some(-2000)), Some(-2.0));
    assert_eq!(milliamps(Some(i64::MIN)), None);
  }
  #[test]
  fn no_internal_battery_is_distinct_from_failure_and_ambiguous_packs() {
    let absent = select(Vec::new(), PowerSource::Ac);
    assert_eq!(absent.state, State::Absent);
    assert_eq!(absent.source, PowerSource::Ac);
    assert_eq!(absent.percent, None);
    assert_eq!(absent.cycles, None);
    assert_eq!(
      select(vec![Snapshot::default(), Snapshot::default()], PowerSource::Battery).state,
      State::Unavailable
    );
    let legacy: Snapshot = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.state, State::Unavailable);
    let data = serde_json::to_value(&absent).unwrap();
    assert!(
      data.as_object().unwrap().keys().all(|key| !["name", "serial", "id"].contains(&key.as_str()))
    );
  }
  #[test]
  fn malformed_source_descriptions_do_not_establish_battery_absence() {
    for values in [vec![], vec![("Type", n(1))], vec![("Type", s("Unknown"))]] {
      assert!(description(&dict(&values), PowerSource::Ac).is_err());
    }
    let internal = |present| dict(&[("Type", s("InternalBattery")), ("Is Present", present)]);
    assert!(description(&internal(n(1)), PowerSource::Ac).is_err());
    assert!(description(&internal(b(false)), PowerSource::Ac).unwrap().is_none());
    assert_eq!(
      description(&internal(b(true)), PowerSource::Ac).unwrap().unwrap().state,
      State::Available
    );
    assert!(description(&dict(&[("Type", s("UPS"))]), PowerSource::Ups).unwrap().is_none());
  }
  #[test]
  fn notifications_are_coalesced_fallback_is_bounded_and_age_increases() {
    let now = Instant::now();
    assert!(due(None, false, now));
    assert!(!due(Some(now), true, now + Duration::from_millis(999)));
    assert!(due(Some(now), true, now + Duration::from_secs(1)));
    assert!(!due(Some(now), false, now + Duration::from_secs(29)));
    assert!(due(Some(now), false, now + REFRESH));
    let old = now - Duration::from_secs(61);
    let cache = Reader(Arc::new(Mutex::new(Cached {
      snapshot: Snapshot::default(),
      acquired: Some(old),
      registry_acquired: Some(old),
    })));
    assert!(cache.snapshot().age_ms.unwrap() > STALE_MS);
    assert!(cache.snapshot().registry_age_ms.unwrap() > STALE_MS);
    let dirty = AtomicBool::new(false);
    unsafe { changed((&dirty as *const AtomicBool).cast_mut().cast()) };
    assert!(dirty.load(Ordering::Acquire));
  }
  #[test]
  fn registry_pairs_are_atomic_and_cannot_fill_in_a_different_power_state() {
    let registry = Registry {
      cycles: Some(150),
      voltage_v: Some(12.0),
      current_a: Some(-2.0),
      charging: Some(false),
      external: Some(false),
    };
    let mut snapshot = Snapshot {
      state: State::Available,
      charging: Some(false),
      source: PowerSource::Battery,
      ..Default::default()
    };
    enrich(&mut snapshot, &registry);
    assert_eq!(snapshot.cycles, Some(150));
    assert_eq!(snapshot.electrical_power_w, Some(-24.0));
    snapshot.source = PowerSource::Ac;
    snapshot.voltage_v = None;
    enrich(&mut snapshot, &registry);
    assert_eq!(snapshot.current_a, None);
    assert_eq!(snapshot.electrical_power_w, None);
    // Never multiply public-source current with registry-source voltage.
    snapshot.current_a = Some(1.0);
    snapshot.voltage_v = None;
    enrich(&mut snapshot, &Registry { voltage_v: Some(12.0), ..Default::default() });
    assert_eq!(snapshot.electrical_power_w, None);
    let unavailable = Reader::default().snapshot();
    assert_eq!(unavailable.state, State::Unavailable);
    assert_eq!(unavailable.age_ms, None);
    assert_eq!(unavailable.registry_age_ms, None);
    assert_eq!(unavailable.percent, None);
  }

  #[test]
  #[ignore = "requires real native battery APIs; run explicitly without sudo"]
  fn native_snapshot_and_worker_cleanup() {
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let before = Instant::now();
    let worker = Worker::start().unwrap();
    let reader = worker.reader.clone();
    while reader.snapshot().age_ms.is_none() && before.elapsed() < Duration::from_secs(5) {
      thread::sleep(Duration::from_millis(50));
    }
    let snapshot = reader.snapshot();
    assert_ne!(snapshot.state, State::Unavailable);
    assert!(snapshot.age_ms.is_some());
    println!("{}", serde_json::to_string(&snapshot).unwrap());
    let stop = Instant::now();
    drop(worker);
    assert!(stop.elapsed() < Duration::from_secs(2));
    println!("battery worker cleanup {}ms", stop.elapsed().as_millis());
  }
}
