//! Physical block-device I/O and independently refreshed, rootless NVMe health.

use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void};
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

use core_foundation::base::{CFAllocatorRef, CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{
  CFDictionary, CFDictionaryRef, CFMutableDictionary, CFMutableDictionaryRef,
};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation::uuid::{
  CFUUID, CFUUIDBytes, CFUUIDCreateFromString, CFUUIDGetUUIDBytes, CFUUIDRef,
};
use serde::Serialize;

/// Counters since a physical block-storage driver was instantiated, not NAND writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
pub struct DiskMetric {
  /// Driver and whole-media registry IDs identify this connection generation.
  pub id: String,
  /// Whole-device BSD name, such as `disk0`; APFS volumes are not separate disks.
  pub name: String,
  pub model: Option<String>,
  pub internal: Option<bool>,
  pub size_bytes: Option<u64>,
  pub read_bytes: Option<u64>,
  pub write_bytes: Option<u64>,
  pub read_operations: Option<u64>,
  pub write_operations: Option<u64>,
  pub read_bytes_per_sec: Option<f64>,
  pub write_bytes_per_sec: Option<f64>,
  pub read_iops: Option<f64>,
  pub write_iops: Option<f64>,
  pub health: DiskHealth,
}

/// Health availability is separate from both I/O activity and the critical-warning value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
#[serde(rename_all = "lowercase")]
pub enum DiskHealthState {
  Available,
  Unsupported,
  #[default]
  Unavailable,
}

/// Independently sampled health; add the containing sample's age when displaying freshness.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
pub struct DiskHealth {
  pub state: DiskHealthState,
  /// Age at completion of the I/O snapshot; `None` until health has been read successfully.
  pub age_ms: Option<u64>,
  pub nvme: Option<NvmeHealth>,
}

/// NVMe health-log counters are lossless decimal strings, including all 128 bits.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
pub struct NvmeHealth {
  pub critical_warning: u8,
  /// Manufacturer estimate of endurance used; may exceed 100, never remaining years.
  pub percentage_used: u8,
  pub available_spare_percent: Option<u8>,
  pub spare_threshold_percent: Option<u8>,
  pub temperature_celsius: Option<f64>,
  /// Host data units, each 512,000 bytes; not physical NAND write amplification.
  pub data_units_read: String,
  pub data_units_written: String,
  pub power_cycles: String,
  pub power_on_hours: String,
  pub unsafe_shutdowns: String,
  pub media_errors: String,
  pub error_log_entries: String,
}

type Baseline = Option<(Instant, Vec<DiskMetric>)>;

#[derive(Default)]
pub(crate) struct DiskSampler {
  baseline: Baseline,
  latest: Option<Vec<DiskMetric>>,
  health: HealthSampler,
}

impl DiskSampler {
  pub(crate) fn latest(&self) -> Option<&[DiskMetric]> {
    self.latest.as_deref()
  }

  pub(crate) fn begin(&mut self) -> Baseline {
    self.latest = None;
    self.baseline.take()
  }

  pub(crate) fn sample(&mut self, baseline: Baseline, vm: bool) {
    let Some(mut devices) = read_devices() else { return };
    self.health.sample(&mut devices, Instant::now(), vm);
    self.record(
      devices.into_iter().map(|device| device.metric).collect(),
      Instant::now(),
      baseline,
    );
  }

  fn record(&mut self, mut current: Vec<DiskMetric>, now: Instant, baseline: Baseline) {
    current.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    for disk in &mut current {
      disk.read_bytes_per_sec = None;
      disk.write_bytes_per_sec = None;
      disk.read_iops = None;
      disk.write_iops = None;
      let Some((then, previous)) = &baseline else { continue };
      let Some(seconds) = now.checked_duration_since(*then).filter(|d| !d.is_zero()) else {
        continue;
      };
      let Some(old) = previous.iter().find(|old| old.id == disk.id) else { continue };
      let pairs = [
        (disk.read_bytes, old.read_bytes),
        (disk.write_bytes, old.write_bytes),
        (disk.read_operations, old.read_operations),
        (disk.write_operations, old.write_operations),
      ];
      // A missing field or reset invalidates this device's complete interval, never a fake zero.
      let changes: Option<Vec<_>> = pairs.into_iter().map(|(a, b)| a?.checked_sub(b?)).collect();
      if let Some(changes) = changes {
        let rates: Vec<_> = changes.into_iter().map(|n| n as f64 / seconds.as_secs_f64()).collect();
        disk.read_bytes_per_sec = Some(rates[0]);
        disk.write_bytes_per_sec = Some(rates[1]);
        disk.read_iops = Some(rates[2]);
        disk.write_iops = Some(rates[3]);
      }
    }
    self.baseline = Some((now, current.clone()));
    self.latest = Some(current);
  }

  #[cfg(test)]
  pub(crate) fn fixture() -> Self {
    let mut sampler = Self::default();
    sampler.record(
      vec![DiskMetric { id: "1:2".into(), name: "disk0".into(), ..Default::default() }],
      Instant::now(),
      None,
    );
    sampler
  }
}

type Dictionary = CFDictionary<CFString, CFType>;

#[link(name = "IOKit", kind = "framework")]
#[rustfmt::skip]
unsafe extern "C" {
  fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
  fn IOServiceGetMatchingServices(port: u32, matching: CFDictionaryRef, iterator: *mut u32) -> i32;
  fn IOIteratorNext(iterator: u32) -> u32;
  fn IOIteratorIsValid(iterator: u32) -> u32;
  fn IOObjectConformsTo(object: u32, class: *const c_char) -> u32;
  fn IOObjectRelease(object: u32) -> u32;
  fn IORegistryEntryGetParentEntry(object: u32, plane: *const c_char, parent: *mut u32) -> i32;
  fn IORegistryEntryGetRegistryEntryID(object: u32, id: *mut u64) -> i32;
  fn IORegistryEntryCreateCFProperties(object: u32, properties: *mut CFMutableDictionaryRef, allocator: CFAllocatorRef, options: u32) -> i32;
  fn IOCreatePlugInInterfaceForService(object: u32, user: CFUUIDRef, interface: CFUUIDRef, plugin: *mut *mut *mut Unknown, score: *mut i32) -> i32;
  fn IODestroyPlugInInterface(plugin: *mut *mut Unknown) -> i32;
}

struct Io(u32);

impl Drop for Io {
  fn drop(&mut self) {
    if self.0 != 0 {
      unsafe {
        IOObjectRelease(self.0);
      }
    }
  }
}

fn parent(object: &Io) -> Option<Io> {
  let mut result = 0;
  let status =
    unsafe { IORegistryEntryGetParentEntry(object.0, c"IOService".as_ptr(), &mut result) };
  let result = Io(result);
  (status == 0 && result.0 != 0).then_some(result)
}

fn properties(object: &Io) -> Option<Dictionary> {
  let mut result: CFMutableDictionaryRef = null_mut();
  let status = unsafe { IORegistryEntryCreateCFProperties(object.0, &mut result, null_mut(), 0) };
  if result.is_null() {
    return None;
  }
  let dictionary: Dictionary = unsafe { TCFType::wrap_under_create_rule(result.cast_const()) };
  (status == 0).then_some(dictionary)
}

fn value(dictionary: &Dictionary, name: &str) -> Option<CFType> {
  dictionary.find(CFString::new(name)).map(|value| (*value).clone())
}

fn number(dictionary: &Dictionary, name: &str) -> Option<u64> {
  let number = value(dictionary, name)?.downcast::<CFNumber>()?;
  // Negative, fractional and out-of-range values must not become a huge counter.
  let integer = number.to_i64()?;
  (number.to_f64()? == integer as f64).then(|| u64::try_from(integer).ok()).flatten()
}

fn string(dictionary: &Dictionary, name: &str) -> Option<String> {
  let text = value(dictionary, name)?.downcast::<CFString>()?.to_string();
  let text = text.trim();
  (!text.is_empty() && text.len() <= 256).then(|| text.to_owned())
}

fn boolean(dictionary: &Dictionary, name: &str) -> Option<bool> {
  value(dictionary, name)?.downcast::<CFBoolean>().map(bool::from)
}

fn nested(dictionary: &Dictionary, name: &str) -> Dictionary {
  value(dictionary, name)
    .and_then(|value| value.downcast::<CFDictionary>())
    .map(|dictionary| unsafe { Dictionary::wrap_under_get_rule(dictionary.as_concrete_TypeRef()) })
    .unwrap_or_else(|| CFDictionary::from_CFType_pairs(&[]))
}

fn registry_id(object: &Io) -> Option<u64> {
  let mut id = 0;
  (unsafe { IORegistryEntryGetRegistryEntryID(object.0, &mut id) } == 0 && id != 0).then_some(id)
}

struct Device {
  metric: DiskMetric,
  driver_id: u64,
  smart: Option<Io>,
  smart_known: bool,
}

fn virtual_provider(properties: &Dictionary) -> bool {
  string(&nested(properties, "Protocol Characteristics"), "Physical Interconnect").as_deref()
    == Some("Virtual Interface")
}

fn find_smart(mut node: Io) -> Result<Option<Io>, ()> {
  // Walk only a bounded provider ancestry; a malformed tree must not loop forever.
  for _ in 0..16 {
    let properties = properties(&node).ok_or(())?;
    if let Some(capable) = value(&properties, "NVMe SMART Capable")
      && bool::from(capable.downcast::<CFBoolean>().ok_or(())?)
    {
      return Ok(Some(node));
    }
    let Some(next) = parent(&node) else { return Ok(None) };
    node = next;
  }
  Err(())
}

fn read_devices() -> Option<Vec<Device>> {
  let matching = unsafe { IOServiceMatching(c"IOMedia".as_ptr()) };
  if matching.is_null() {
    return None;
  }
  let mut matching: CFMutableDictionary<CFString, CFType> =
    unsafe { TCFType::wrap_under_create_rule(matching) };
  let whole = CFDictionary::from_CFType_pairs(&[(
    CFString::new("Whole"),
    CFBoolean::true_value().as_CFType(),
  )]);
  matching.set(CFString::new("IOPropertyMatch"), whole.as_CFType());
  let mut iterator = 0;
  // Matching consumes its dictionary, even on failure.
  let matching = std::mem::ManuallyDrop::new(matching);
  let status = unsafe {
    IOServiceGetMatchingServices(0, matching.as_concrete_TypeRef().cast(), &mut iterator)
  };
  let iterator = Io(iterator);
  if status != 0 || iterator.0 == 0 {
    return None;
  }
  let mut devices = Vec::new();
  let mut visited = 0;
  loop {
    let media = Io(unsafe { IOIteratorNext(iterator.0) });
    if media.0 == 0 {
      break;
    }
    visited += 1;
    if visited > 4096 {
      return None;
    }
    let media_properties = properties(&media)?;
    if boolean(&media_properties, "Whole") != Some(true) {
      continue;
    }
    let driver = parent(&media)?;
    if unsafe { IOObjectConformsTo(driver.0, c"IOBlockStorageDriver".as_ptr()) } == 0 {
      continue;
    }
    let provider = parent(&driver)?;
    let provider_properties = properties(&provider)?;
    let protocol = nested(&provider_properties, "Protocol Characteristics");
    if virtual_provider(&provider_properties) {
      continue;
    }
    let Some(name) = string(&media_properties, "BSD Name") else { continue };
    let size_bytes = number(&media_properties, "Size");
    if size_bytes == Some(0) {
      continue;
    }
    let driver_id = registry_id(&driver)?;
    let media_id = registry_id(&media)?;
    let stats = nested(&properties(&driver)?, "Statistics");
    let location = string(&protocol, "Physical Interconnect Location");
    let metric = DiskMetric {
      id: format!("{driver_id}:{media_id}"),
      name,
      model: string(&nested(&provider_properties, "Device Characteristics"), "Product Name"),
      internal: location.and_then(|location| match location.as_str() {
        "Internal" => Some(true),
        "External" => Some(false),
        _ => None,
      }),
      size_bytes,
      read_bytes: number(&stats, "Bytes (Read)"),
      write_bytes: number(&stats, "Bytes (Write)"),
      read_operations: number(&stats, "Operations (Read)"),
      write_operations: number(&stats, "Operations (Write)"),
      ..Default::default()
    };
    let smart = find_smart(provider);
    let smart_known = smart.is_ok();
    devices.push(Device { metric, driver_id, smart: smart.ok().flatten(), smart_known });
    if devices.len() > 1024 {
      return None;
    }
  }
  if unsafe { IOIteratorIsValid(iterator.0) } == 0 {
    return None;
  }
  devices.sort_by(|a, b| (a.driver_id, &a.metric.id).cmp(&(b.driver_id, &b.metric.id)));
  devices.dedup_by_key(|device| device.driver_id);
  Some(devices)
}

// Public CFPlugInCOM.h / NVMeSMARTLibExternal.h prefix layouts, including the reserved slot.
#[repr(C)]
struct Unknown {
  reserved: *mut c_void,
  query: Option<unsafe extern "C" fn(*mut c_void, CFUUIDBytes, *mut *mut c_void) -> i32>,
  add_ref: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
  release: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
}

#[repr(C)]
struct Smart {
  unknown: Unknown,
  version: u16,
  revision: u16,
  read: Option<unsafe extern "C" fn(*mut c_void, *mut u8) -> i32>,
}

struct Com {
  pointer: *mut *mut Unknown,
  plugin: bool,
}

impl Drop for Com {
  fn drop(&mut self) {
    if self.pointer.is_null() {
      return;
    }
    unsafe {
      if self.plugin {
        IODestroyPlugInInterface(self.pointer);
      } else if !(*self.pointer).is_null()
        && let Some(release) = (**self.pointer).release
      {
        release(self.pointer.cast());
      }
    }
  }
}

fn uuid(text: &str) -> Option<CFUUID> {
  let text = CFString::new(text);
  let raw = unsafe { CFUUIDCreateFromString(null_mut(), text.as_concrete_TypeRef()) };
  (!raw.is_null()).then(|| unsafe { TCFType::wrap_under_create_rule(raw) })
}

fn read_nvme(service: &Io) -> Option<NvmeHealth> {
  let user = uuid("AA0FA6F9-C2D6-457F-B10B-59A13253292F")?;
  let interface = uuid("C244E858-109C-11D4-91D4-0050E4C6426F")?;
  let smart_id = uuid("CCD1DB19-FD9A-4DAF-BF95-12454B230AB6")?;
  let mut plugin = Com { pointer: null_mut(), plugin: true };
  let mut score = 0;
  let status = unsafe {
    IOCreatePlugInInterfaceForService(
      service.0,
      user.as_concrete_TypeRef(),
      interface.as_concrete_TypeRef(),
      &mut plugin.pointer,
      &mut score,
    )
  };
  if status != 0 || plugin.pointer.is_null() {
    return None;
  }
  let mut smart = Com { pointer: null_mut(), plugin: false };
  let mut bytes = [0u8; 512];
  unsafe {
    if (*plugin.pointer).is_null() {
      return None;
    }
    let query = (**plugin.pointer).query?;
    let status = query(
      plugin.pointer.cast(),
      CFUUIDGetUUIDBytes(smart_id.as_concrete_TypeRef()),
      (&mut smart.pointer as *mut *mut *mut Unknown).cast(),
    );
    if status != 0 || smart.pointer.is_null() || (*smart.pointer).is_null() {
      return None;
    }
    let table = *smart.pointer as *const Smart;
    if ((*table).read?)(smart.pointer.cast(), bytes.as_mut_ptr()) != 0 {
      return None;
    }
  }
  decode_nvme(&bytes)
}

fn decode_nvme(bytes: &[u8]) -> Option<NvmeHealth> {
  let bytes: &[u8; 512] = bytes.try_into().ok()?;
  let counter =
    |offset| u128::from_le_bytes(bytes[offset..offset + 16].try_into().unwrap()).to_string();
  let kelvin = u16::from_le_bytes([bytes[1], bytes[2]]);
  Some(NvmeHealth {
    critical_warning: bytes[0],
    percentage_used: bytes[5],
    available_spare_percent: (bytes[3] <= 100).then_some(bytes[3]),
    spare_threshold_percent: (bytes[4] <= 100).then_some(bytes[4]),
    temperature_celsius: (kelvin != 0 && kelvin != u16::MAX).then(|| f64::from(kelvin) - 273.15),
    data_units_read: counter(32),
    data_units_written: counter(48),
    power_cycles: counter(112),
    power_on_hours: counter(128),
    unsafe_shutdowns: counter(144),
    media_errors: counter(160),
    error_log_entries: counter(176),
  })
}

const HEALTH_INTERVAL: Duration = Duration::from_secs(60);
type HealthBatch = Option<Vec<(String, Option<NvmeHealth>, Instant)>>;

#[derive(Default)]
struct HealthSampler {
  requests: Option<SyncSender<()>>,
  replies: Option<Receiver<HealthBatch>>,
  pending: bool,
  last_attempt: Option<Instant>,
  cache: HashMap<String, (Option<NvmeHealth>, Instant)>,
}

impl HealthSampler {
  fn receive(&mut self) {
    let Some(replies) = &self.replies else { return };
    match replies.try_recv() {
      Ok(batch) => {
        self.pending = false;
        self.cache =
          batch.unwrap_or_default().into_iter().map(|(id, value, at)| (id, (value, at))).collect();
      }
      Err(TryRecvError::Disconnected) => {
        self.pending = false;
        self.requests = None;
        self.replies = None;
        self.cache.clear();
      }
      Err(TryRecvError::Empty) => {}
    }
  }

  fn sample(&mut self, devices: &mut [Device], now: Instant, vm: bool) {
    self.receive();
    let live: HashSet<_> = devices.iter().map(|device| device.metric.id.clone()).collect();
    self.cache.retain(|id, _| live.contains(id));
    for device in devices.iter_mut() {
      device.metric.health = if vm || !device.smart_known {
        DiskHealth::default()
      } else if device.smart.is_none() {
        DiskHealth { state: DiskHealthState::Unsupported, ..Default::default() }
      } else if let Some((Some(nvme), sampled)) = self.cache.get(&device.metric.id) {
        DiskHealth {
          state: DiskHealthState::Available,
          age_ms: Some(
            now.saturating_duration_since(*sampled).as_millis().min(u64::MAX as u128) as u64
          ),
          nvme: Some(nvme.clone()),
        }
      } else {
        DiskHealth::default()
      };
    }
    if vm
      || !devices.iter().any(|device| device.smart.is_some())
      || self.pending
      || self.last_attempt.is_some_and(|then| now.saturating_duration_since(then) < HEALTH_INTERVAL)
    {
      return;
    }
    // Record failures too. Never retry a stuck request or spawn replacement threads for it.
    self.last_attempt = Some(now);
    if self.requests.is_none() {
      let (requests, receive) = mpsc::sync_channel(1);
      let (send, replies) = mpsc::channel();
      if std::thread::Builder::new()
        .name("macmon-disk-health".into())
        .spawn(move || {
          while receive.recv().is_ok() {
            let batch = read_devices().map(|devices| {
              devices
                .into_iter()
                .filter_map(|device| {
                  let service = device.smart?;
                  let nvme = read_nvme(&service);
                  Some((device.metric.id, nvme, Instant::now()))
                })
                .collect()
            });
            if send.send(batch).is_err() {
              break;
            }
          }
        })
        .is_err()
      {
        return;
      }
      self.requests = Some(requests);
      self.replies = Some(replies);
    }
    self.pending = self.requests.as_ref().is_some_and(|requests| requests.try_send(()).is_ok());
  }
}

#[cfg(test)]
mod tests {
  use std::mem::{offset_of, size_of};
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;

  fn counter(id: &str, read: u64, write: u64, reads: u64, writes: u64) -> DiskMetric {
    DiskMetric {
      id: id.into(),
      name: "disk0".into(),
      read_bytes: Some(read),
      write_bytes: Some(write),
      read_operations: Some(reads),
      write_operations: Some(writes),
      ..Default::default()
    }
  }

  fn observe(sampler: &mut DiskSampler, current: DiskMetric, now: Instant) -> DiskMetric {
    let baseline = sampler.begin();
    sampler.record(vec![current], now, baseline);
    sampler.latest().unwrap()[0].clone()
  }

  fn rates(metric: &DiskMetric) -> [Option<f64>; 4] {
    [metric.read_bytes_per_sec, metric.write_bytes_per_sec, metric.read_iops, metric.write_iops]
  }

  #[test]
  fn measured_rates_keep_idle_zero_and_large_counters_without_initial_spikes() {
    let now = Instant::now();
    let large = (1u64 << 53) + 100;
    let mut sampler = DiskSampler::default();
    assert!(sampler.latest().is_none());
    assert_eq!(rates(&observe(&mut sampler, counter("1:2", large, 0, 2, 0), now)), [None; 4]);
    let current = observe(
      &mut sampler,
      counter("1:2", large + 500, 1000, 7, 10),
      now + Duration::from_millis(2500),
    );
    assert_eq!(rates(&current), [Some(200.0), Some(400.0), Some(2.0), Some(4.0)]);
    let idle = observe(&mut sampler, current.clone(), now + Duration::from_secs(4));
    assert_eq!(rates(&idle), [Some(0.0); 4]);
    let same_time = observe(&mut sampler, current, now + Duration::from_secs(4));
    assert_eq!(rates(&same_time), [None; 4]);
  }

  #[test]
  fn reset_missing_field_reconnect_and_failed_acquisition_replace_all_rate_baselines() {
    let now = Instant::now();
    let mut sampler = DiskSampler::default();
    observe(&mut sampler, counter("1:2", 100, 100, 100, 100), now);
    let reset =
      observe(&mut sampler, counter("1:2", 99, 200, 200, 200), now + Duration::from_secs(1));
    assert_eq!(rates(&reset), [None; 4]);
    let recovered =
      observe(&mut sampler, counter("1:2", 199, 300, 300, 300), now + Duration::from_secs(2));
    assert_eq!(rates(&recovered), [Some(100.0); 4]);
    let mut missing = recovered;
    missing.write_operations = None;
    assert_eq!(rates(&observe(&mut sampler, missing, now + Duration::from_secs(3))), [None; 4]);
    assert_eq!(
      rates(&observe(
        &mut sampler,
        counter("1:2", 400, 400, 400, 400),
        now + Duration::from_secs(4)
      )),
      [None; 4]
    );
    assert_eq!(
      rates(&observe(
        &mut sampler,
        counter("1:3", 1000, 1000, 1000, 1000),
        now + Duration::from_secs(5)
      )),
      [None; 4]
    );
    // Beginning an acquisition clears public data. An early CPU/SMC error drops this baseline.
    drop(sampler.begin());
    assert!(sampler.latest().is_none());
    let after_failure =
      observe(&mut sampler, counter("1:3", 5000, 5000, 5000, 5000), now + Duration::from_secs(6));
    assert_eq!(rates(&after_failure), [None; 4]);
    let baseline = sampler.begin();
    sampler.record(vec![], now + Duration::from_secs(7), baseline);
    assert_eq!(sampler.latest(), Some([].as_slice()));
  }

  #[test]
  fn cf_properties_reject_wrong_types_negative_fractional_and_virtual_sources() {
    let large = (1i64 << 53) + 123;
    let properties = Dictionary::from_CFType_pairs(&[
      (CFString::new("counter"), CFNumber::from(large).as_CFType()),
      (CFString::new("negative"), CFNumber::from(-1i64).as_CFType()),
      (CFString::new("fractional"), CFNumber::from(1.5f64).as_CFType()),
      (CFString::new("string"), CFString::new("100").as_CFType()),
      (CFString::new("boolean"), CFBoolean::true_value().as_CFType()),
    ]);
    assert_eq!(number(&properties, "counter"), Some(large as u64));
    for key in ["negative", "fractional", "string", "boolean", "missing"] {
      assert_eq!(number(&properties, key), None);
    }
    assert_eq!(boolean(&properties, "counter"), None);
    assert_eq!(boolean(&properties, "boolean"), Some(true));
    assert_eq!(string(&properties, "counter"), None);
    assert!(nested(&properties, "string").is_empty());
    let protocol = Dictionary::from_CFType_pairs(&[(
      CFString::new("Physical Interconnect"),
      CFString::new("Virtual Interface").as_CFType(),
    )]);
    let provider = Dictionary::from_CFType_pairs(&[(
      CFString::new("Protocol Characteristics"),
      protocol.as_CFType(),
    )]);
    assert!(virtual_provider(&provider));
    assert!(!virtual_provider(&properties));
  }

  #[test]
  fn nvme_decoding_preserves_128_bits_and_estimated_wear_above_100() {
    let mut bytes = [0u8; 512];
    bytes[0] = 0x84;
    bytes[1..3].copy_from_slice(&311u16.to_le_bytes());
    bytes[3] = 100;
    bytes[4] = 99;
    bytes[5] = 201;
    for offset in [32, 48, 112, 128, 144, 160, 176] {
      bytes[offset..offset + 16].copy_from_slice(&u128::MAX.to_le_bytes());
    }
    let health = decode_nvme(&bytes).unwrap();
    assert_eq!(health.critical_warning, 0x84);
    assert_eq!(health.percentage_used, 201);
    assert!((health.temperature_celsius.unwrap() - 37.85).abs() < 1e-9);
    assert_eq!(health.data_units_written, u128::MAX.to_string());
    assert_eq!(health.media_errors, u128::MAX.to_string());
    let json = serde_json::to_value(health).unwrap();
    assert_eq!(json["data_units_read"], u128::MAX.to_string());
    assert_eq!(json["power_on_hours"], u128::MAX.to_string());
    assert!(decode_nvme(&bytes[..511]).is_none());
    for kelvin in [0u16, u16::MAX] {
      bytes[1..3].copy_from_slice(&kelvin.to_le_bytes());
      assert_eq!(decode_nvme(&bytes).unwrap().temperature_celsius, None);
    }
    bytes[3] = 255;
    bytes[4] = 101;
    let invalid = decode_nvme(&bytes).unwrap();
    assert_eq!((invalid.available_spare_percent, invalid.spare_threshold_percent), (None, None));
    for (state, expected) in [
      (DiskHealthState::Available, "available"),
      (DiskHealthState::Unsupported, "unsupported"),
      (DiskHealthState::Unavailable, "unavailable"),
    ] {
      assert_eq!(serde_json::to_value(state).unwrap(), expected);
    }
  }

  fn health_fixture() -> (HealthSampler, mpsc::Receiver<()>, mpsc::Sender<HealthBatch>) {
    let (requests, receive) = mpsc::sync_channel(1);
    let (send, replies) = mpsc::channel();
    (
      HealthSampler { requests: Some(requests), replies: Some(replies), ..Default::default() },
      receive,
      send,
    )
  }

  fn device(id: &str) -> Device {
    Device { metric: counter(id, 0, 0, 0, 0), driver_id: 1, smart: Some(Io(0)), smart_known: true }
  }

  #[test]
  fn pending_health_cannot_spawn_repeated_jobs_and_failures_are_throttled() {
    let now = Instant::now();
    let (mut health, requests, replies) = health_fixture();
    let mut devices = [device("1:2")];
    health.sample(&mut devices, now, false);
    requests.try_recv().unwrap();
    assert!(health.pending);
    health.sample(&mut devices, now + Duration::from_secs(120), false);
    assert!(requests.try_recv().is_err(), "A blocked read never creates replacement workers/jobs");
    drop((health, requests, replies));

    // A separate fixture keeps the failure/retry test's clock monotonic, too.
    let (mut health, requests, replies) = health_fixture();
    health.sample(&mut devices, now, false);
    requests.try_recv().unwrap();
    replies.send(None).unwrap();
    // A failure completing within the original interval cannot be retried until 60 seconds.
    health.sample(&mut devices, now + Duration::from_secs(59), false);
    assert!(!health.pending && requests.try_recv().is_err());
    assert_eq!(devices[0].metric.health.state, DiskHealthState::Unavailable);
    health.sample(&mut devices, now + Duration::from_secs(60), false);
    requests.try_recv().unwrap();
    assert!(health.pending);
  }

  #[test]
  fn health_age_old_replies_reconnect_vm_and_unsupported_sources_are_explicit() {
    let now = Instant::now();
    let (mut health, requests, replies) = health_fixture();
    let mut devices = [device("1:2")];
    health.sample(&mut devices, now, false);
    requests.try_recv().unwrap();
    replies
      .send(Some(vec![(
        "1:2".into(),
        Some(NvmeHealth { percentage_used: 3, ..Default::default() }),
        now,
      )]))
      .unwrap();
    health.sample(&mut devices, now + Duration::from_secs(2), false);
    assert_eq!(devices[0].metric.health.age_ms, Some(2000));
    assert_eq!(devices[0].metric.health.nvme.as_ref().unwrap().percentage_used, 3);
    health.sample(&mut devices, now + Duration::from_secs(3), true);
    assert_eq!(devices[0].metric.health, DiskHealth::default());
    devices[0].metric.id = "1:3".into();
    replies.send(Some(vec![("1:2".into(), Some(NvmeHealth::default()), now)])).unwrap();
    health.sample(&mut devices, now + Duration::from_secs(4), false);
    assert!(health.cache.is_empty());
    assert_eq!(devices[0].metric.health, DiskHealth::default());
    devices[0].smart = None;
    devices[0].smart_known = false;
    health.sample(&mut devices, now + Duration::from_secs(5), false);
    assert_eq!(devices[0].metric.health.state, DiskHealthState::Unavailable);
    devices[0].smart_known = true;
    health.sample(&mut devices, now + Duration::from_secs(6), false);
    assert_eq!(devices[0].metric.health.state, DiskHealthState::Unsupported);
    assert!(devices[0].metric.health.nvme.is_none());
    assert!(requests.try_recv().is_err());
  }

  #[test]
  fn com_prefix_layout_and_release_match_public_sdk() {
    static RELEASES: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn release(_: *mut c_void) -> u32 {
      RELEASES.fetch_add(1, Ordering::SeqCst);
      0
    }
    assert_eq!(size_of::<CFUUIDBytes>(), 16);
    assert_eq!(size_of::<Unknown>(), 32);
    assert_eq!(offset_of!(Unknown, query), 8);
    assert_eq!(offset_of!(Smart, read), 40);
    let mut table =
      Unknown { reserved: null_mut(), query: None, add_ref: None, release: Some(release) };
    let mut pointer = &mut table as *mut Unknown;
    drop(Com { pointer: &mut pointer, plugin: false });
    assert_eq!(RELEASES.load(Ordering::SeqCst), 1);
    let id = uuid("CCD1DB19-FD9A-4DAF-BF95-12454B230AB6").unwrap();
    let bytes = unsafe { CFUUIDGetUUIDBytes(id.as_concrete_TypeRef()) };
    assert_eq!(
      [bytes.byte0, bytes.byte1, bytes.byte2, bytes.byte3, bytes.byte15],
      [0xcc, 0xd1, 0xdb, 0x19, 0xb6]
    );
  }

  #[test]
  #[ignore = "opt-in read-only physical Mac disk and NVMe SMART check; no write load"]
  fn rootless_disk_io_and_health_hardware_probe() {
    let mut sampler = DiskSampler::default();
    let baseline = sampler.begin();
    sampler.sample(baseline, false);
    let first = sampler.latest().unwrap();
    assert!(!first.is_empty());
    assert!(first.iter().all(|disk| rates(disk) == [None; 4]));
    for _ in 0..30 {
      std::thread::sleep(Duration::from_millis(100));
      let baseline = sampler.begin();
      sampler.sample(baseline, false);
      if sampler
        .latest()
        .unwrap()
        .iter()
        .any(|disk| disk.health.state == DiskHealthState::Available)
      {
        break;
      }
    }
    println!(
      "{}",
      serde_json::json!({"uid": unsafe { libc::geteuid() }, "disks": sampler.latest()})
    );
    assert!(
      sampler.latest().unwrap().iter().any(|disk| disk.health.state == DiskHealthState::Available)
    );
  }
}
