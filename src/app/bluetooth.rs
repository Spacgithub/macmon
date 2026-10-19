//! Cached, read-only Bluetooth reports. No discovery, pairing or radio operations.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const INTERVAL_MS: u64 = 30_000;
const STALE_MS: u64 = 90_000;
const MAX_OUTPUT: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
  #[default]
  Warming,
  Ready,
  Stale,
  Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Battery {
  pub component: String,
  pub percent: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Device {
  /// Random-keyed, collector-session-only identity; never the Bluetooth address or serial.
  pub id: String,
  pub name: String,
  pub kind: String,
  pub batteries: Vec<Battery>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Snapshot {
  pub state: State,
  pub controller: Option<bool>,
  pub sampled_at: Option<String>,
  pub age_ms: Option<u64>,
  pub devices: Vec<Device>,
}

impl Snapshot {
  pub(crate) fn aged(&self, elapsed_ms: u64) -> Self {
    let mut value = self.clone();
    value.age_ms = value.age_ms.map(|age| age.saturating_add(elapsed_ms));
    if value.state == State::Ready && value.age_ms.is_some_and(|age| age > STALE_MS) {
      value.state = State::Stale;
    }
    value
  }
}

#[derive(Default)]
struct Cache {
  snapshot: Snapshot,
  sampled: Option<Instant>,
}

impl Cache {
  fn update(&mut self, report: io::Result<Snapshot>) {
    match report {
      Ok(mut snapshot) => {
        snapshot.sampled_at = Some(chrono::Utc::now().to_rfc3339());
        snapshot.age_ms = Some(0);
        self.snapshot = snapshot;
        self.sampled = Some(Instant::now());
      }
      Err(_) => {
        self.snapshot.state =
          if self.sampled.is_some() { State::Stale } else { State::Unavailable };
      }
    }
  }
  fn latest(&self) -> Snapshot {
    self.snapshot.aged(self.sampled.map_or(0, |time| millis(time.elapsed())))
  }
}

fn millis(duration: Duration) -> u64 {
  duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// Owned by source::run. Drop interrupts acquisition, kills/reaps its child and joins its thread.
pub(crate) struct Worker {
  cache: Arc<Mutex<Cache>>,
  stop: Arc<(Mutex<bool>, Condvar)>,
  thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub(crate) struct Reader(Arc<Mutex<Cache>>);
impl Reader {
  pub(crate) fn latest(&self) -> Snapshot {
    self.0.lock().unwrap().latest()
  }
}

impl Worker {
  pub(crate) fn start() -> io::Result<Self> {
    Self::spawn(|stop, ids| read_report(stop).and_then(|bytes| parse(&bytes, ids)))
  }

  fn spawn(
    mut collect: impl FnMut(&(Mutex<bool>, Condvar), &RandomState) -> io::Result<Snapshot>
    + Send
    + 'static,
  ) -> io::Result<Self> {
    let cache = Arc::new(Mutex::new(Cache::default()));
    let stop = Arc::new((Mutex::new(false), Condvar::new()));
    let (output, flag) = (Arc::clone(&cache), Arc::clone(&stop));
    let thread = thread::Builder::new().name("bluetooth-report".into()).spawn(move || {
      let ids = RandomState::new();
      loop {
        if *flag.0.lock().unwrap() {
          break;
        }
        let report = collect(&flag, &ids);
        output.lock().unwrap().update(report);
        let (stopped, _) = flag
          .1
          .wait_timeout_while(flag.0.lock().unwrap(), Duration::from_millis(INTERVAL_MS), |stop| {
            !*stop
          })
          .unwrap();
        if *stopped {
          break;
        }
      }
    })?;
    Ok(Self { cache, stop, thread: Some(thread) })
  }

  pub(crate) fn reader(&self) -> Reader {
    Reader(Arc::clone(&self.cache))
  }
}

impl Drop for Worker {
  fn drop(&mut self) {
    *self.stop.0.lock().unwrap() = true;
    self.stop.1.notify_all();
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
  }
}

struct ReportChild(Child, bool);
impl Drop for ReportChild {
  fn drop(&mut self) {
    // A private process group also prevents reporter descendants from surviving cancellation.
    if !self.1 {
      unsafe {
        libc::kill(-(self.0.id() as libc::pid_t), libc::SIGKILL);
      }
    }
    let _ = self.0.wait();
  }
}

fn read_report(stop: &(Mutex<bool>, Condvar)) -> io::Result<Vec<u8>> {
  // The basic report supplies an address only for ephemeral identity matching. The parser
  // immediately drops all addresses, serials, firmware and disconnected-device information.
  let mut command = Command::new("/usr/sbin/system_profiler");
  command
    // Our four-second parent deadline fires before system_profiler's own partial-report timeout.
    .args(["SPBluetoothDataType", "-json", "-detailLevel", "basic", "-timeout", "5"])
    .env("LC_ALL", "C")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .process_group(0);
  capture(command, stop, TIMEOUT)
}

fn capture(
  mut command: Command,
  stop: &(Mutex<bool>, Condvar),
  timeout: Duration,
) -> io::Result<Vec<u8>> {
  let mut child = ReportChild(command.spawn()?, false);
  let mut stdout = child.0.stdout.take().ok_or_else(|| io::Error::other("missing report pipe"))?;
  let fd = stdout.as_raw_fd();
  let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
  if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
    return Err(io::Error::last_os_error());
  }
  let started = Instant::now();
  let mut output = Vec::new();
  let mut exited: Option<std::process::ExitStatus> = None;
  loop {
    if *stop.0.lock().unwrap() || started.elapsed() > timeout {
      return Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Bluetooth report cancelled or timed out",
      ));
    }
    let mut bytes = [0; 8192];
    match stdout.read(&mut bytes) {
      Ok(0) if exited.is_some() => {
        // The report and its pipe are finished; do not signal a reaped/reusable process ID.
        child.1 = true;
        return if exited.unwrap().success() {
          Ok(output)
        } else {
          Err(io::Error::other("Bluetooth report failed"))
        };
      }
      Ok(count) => {
        if output.len().saturating_add(count) > MAX_OUTPUT {
          return Err(io::Error::other("Bluetooth report too large"));
        }
        output.extend_from_slice(&bytes[..count]);
      }
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
      Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
      Err(error) => return Err(error),
    }
    if exited.is_none() {
      exited = child.0.try_wait()?;
    }
    if output.len() >= MAX_OUTPUT {
      return Err(io::Error::other("Bluetooth report too large"));
    }
    thread::sleep(Duration::from_millis(20));
  }
}

fn text(value: &str, max: usize) -> String {
  value.trim().chars().filter(|character| !character.is_control()).take(max).collect()
}

fn battery(value: &Value) -> Option<u8> {
  let value = value.as_str()?.trim().strip_suffix('%')?.trim();
  let percent: u8 = value.parse().ok()?;
  (percent <= 100).then_some(percent)
}

fn parse(bytes: &[u8], ids: &RandomState) -> io::Result<Snapshot> {
  let root: Value = serde_json::from_slice(bytes)?;
  let reports = root
    .get("SPBluetoothDataType")
    .and_then(Value::as_array)
    .filter(|reports| !reports.is_empty())
    .ok_or_else(|| io::Error::other("missing Bluetooth report"))?;
  let mut result = Snapshot { state: State::Ready, ..Default::default() };
  let mut seen = HashSet::new();
  let mut unnamed_counts = HashMap::<String, usize>::new();
  for report in reports {
    let state = report
      .get("controller_properties")
      .and_then(|controller| controller.get("controller_state"))
      .and_then(Value::as_str);
    let on = match state {
      Some("attrib_on") => true,
      Some("attrib_off") => false,
      _ => return Err(io::Error::other("Bluetooth controller unavailable")),
    };
    result.controller = Some(result.controller.unwrap_or(false) || on);
    let Some(connected) = report.get("device_connected") else {
      continue;
    };
    let connected =
      connected.as_array().ok_or_else(|| io::Error::other("invalid Bluetooth device list"))?;
    if !on {
      continue;
    }
    for entry in connected {
      let entries =
        entry.as_object().ok_or_else(|| io::Error::other("invalid Bluetooth device"))?;
      for (name, device) in entries {
        if !device.is_object() {
          return Err(io::Error::other("invalid Bluetooth device properties"));
        }
        let mut name = text(name, 160);
        let address_name =
          name.chars().filter(|character| !matches!(character, ':' | '-')).collect::<String>();
        if name.is_empty()
          || (address_name.len() == 12 && address_name.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
          name = "Unnamed device".into();
        }
        let mut kind = text(
          device.get("device_minorType").and_then(Value::as_str).unwrap_or("Unknown type"),
          64,
        );
        if kind.is_empty() {
          kind = "Unknown type".into();
        }
        let identity = device
          .get("device_address")
          .and_then(Value::as_str)
          .filter(|address| !address.is_empty())
          .map(|address| address.trim().to_ascii_lowercase())
          .unwrap_or_else(|| {
            let base = format!("{name}\0{kind}");
            let index = unnamed_counts.entry(base.clone()).or_default();
            let identity = format!("{base}\0{index}");
            *index += 1;
            identity
          });
        let id = format!("bt-{:016x}", ids.hash_one(&identity));
        if !seen.insert(id.clone()) {
          continue;
        }
        let batteries = [
          ("main", "device_batteryLevelMain"),
          ("left", "device_batteryLevelLeft"),
          ("right", "device_batteryLevelRight"),
          ("case", "device_batteryLevelCase"),
        ]
        .into_iter()
        .filter_map(|(component, key)| {
          battery(&device[key]).map(|percent| Battery { component: component.into(), percent })
        })
        .collect();
        result.devices.push(Device { id, name, kind, batteries });
      }
    }
  }
  result.devices.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
  Ok(result)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use serde_json::json;

  use super::*;

  fn report(devices: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"SPBluetoothDataType":[{"controller_properties":{"controller_state":"attrib_on","controller_address":"11:22:33:44:55:66"},"device_connected":devices,"device_not_connected":[{"Disconnected private device":{"device_batteryLevelMain":"99%"}}]}]})).unwrap()
  }

  #[test]
  fn connected_only_real_components_and_missing_coverage_are_kept_separate() {
    let parsed = parse(&report(json!([
      {"Mouse":{"device_address":"12:34:56:78:9a:bc","device_minorType":"Mouse","device_batteryLevelMain":"0%"}},
      {"Headphones":{"device_address":"12:34:56:78:9a:bd","device_minorType":"Headphones","device_batteryLevelLeft":"25%","device_batteryLevelRight":"75%","device_batteryLevelCase":"100%","device_serialNumber":"private serial"}},
      {"Keyboard":{"device_minorType":"Keyboard"}}
    ])), &RandomState::new()).unwrap();
    assert_eq!(parsed.devices.len(), 3);
    assert_eq!(
      parsed.devices[0]
        .batteries
        .iter()
        .map(|battery| (battery.component.as_str(), battery.percent))
        .collect::<Vec<_>>(),
      [("left", 25), ("right", 75), ("case", 100)]
    );
    assert!(parsed.devices[1].batteries.is_empty());
    assert_eq!(parsed.devices[2].batteries[0].percent, 0);
    let exported = serde_json::to_string(&parsed).unwrap();
    for private in ["12:34:56", "11:22:33", "private serial", "Disconnected private"] {
      assert!(!exported.contains(private));
    }
  }

  #[test]
  fn percentages_validate_units_and_ranges_without_guessing_missing_zeroes() {
    for (input, expected) in [
      (json!("0%"), Some(0)),
      (json!(" 100% "), Some(100)),
      (json!("101%"), None),
      (json!("-1%"), None),
      (json!("40.5%"), None),
      (json!(50), None),
      (json!("50"), None),
      (Value::Null, None),
    ] {
      assert_eq!(battery(&input), expected, "{input}");
    }
  }

  #[test]
  fn missing_malformed_and_empty_reports_are_distinct_from_no_connections() {
    for invalid in [
      json!({}),
      json!({"SPBluetoothDataType":[]}),
      json!({"SPBluetoothDataType":[{}]}),
      json!({"SPBluetoothDataType":[{"controller_properties":{"controller_state":"new_state"}}]}),
    ] {
      assert!(parse(&serde_json::to_vec(&invalid).unwrap(), &RandomState::new()).is_err());
    }
    for invalid in [json!({}), json!([null]), json!([{"Bad":null}])] {
      assert!(parse(&report(invalid), &RandomState::new()).is_err());
    }
    assert_eq!(parse(&report(json!([])), &RandomState::new()).unwrap().devices.len(), 0);
    let off =
      br#"{"SPBluetoothDataType":[{"controller_properties":{"controller_state":"attrib_off"}}]}"#;
    let parsed = parse(off, &RandomState::new()).unwrap();
    assert_eq!(parsed.controller, Some(false));
    assert_eq!(parsed.state, State::Ready);
    assert!(parsed.devices.is_empty());
  }

  #[test]
  fn opaque_identity_survives_reorder_and_reconnect_but_not_collector_restart() {
    let ids = RandomState::new();
    let a = json!({"Same name":{"device_address":"12:34:56:78:9a:bc"}});
    let b = json!({"Same name":{"device_address":"12:34:56:78:9a:bd"}});
    let before = parse(&report(json!([a.clone(), b.clone()])), &ids).unwrap();
    let after = parse(&report(json!([b.clone(), a.clone()])), &ids).unwrap();
    assert_eq!(before.devices, after.devices);
    assert_ne!(before.devices[0].id, before.devices[1].id);
    assert!(parse(&report(json!([])), &ids).unwrap().devices.is_empty());
    assert_eq!(
      before.devices,
      parse(&report(json!([a.clone(), b.clone()])), &ids).unwrap().devices
    );
    assert_ne!(
      before.devices[0].id,
      parse(&report(json!([a, b])), &RandomState::new()).unwrap().devices[0].id
    );
    let keyboard = json!({"Keyboard":{"device_minorType":"Keyboard"}});
    let named = parse(&report(json!([keyboard.clone()])), &ids).unwrap();
    let reordered =
      parse(&report(json!([{"Mouse":{"device_address":"aa:bb:cc:dd:ee:ff"}}, keyboard])), &ids)
        .unwrap();
    assert_eq!(
      named.devices[0].id,
      reordered.devices.iter().find(|device| device.name == "Keyboard").unwrap().id
    );
    let address_name =
      parse(&report(json!([{"AA:BB:CC:DD:EE:FF":{"device_address":"AA:BB:CC:DD:EE:FF"}}])), &ids)
        .unwrap();
    assert_eq!(address_name.devices[0].name, "Unnamed device");
  }

  #[test]
  fn cache_tracks_staleness_failures_and_recovery_without_faking_disconnections() {
    let mut cache = Cache::default();
    assert_eq!(cache.latest().state, State::Warming);
    cache.update(Err(io::Error::other("failed")));
    assert_eq!(cache.latest().state, State::Unavailable);
    cache.update(parse(
      &report(json!([{"Mouse":{"device_batteryLevelMain":"50%"}}])),
      &RandomState::new(),
    ));
    let before = cache.latest();
    assert_eq!(before.state, State::Ready);
    assert_eq!(before.aged(STALE_MS + 1).state, State::Stale);
    cache.sampled = Some(Instant::now() - Duration::from_secs(4));
    cache.update(Err(io::Error::other("failed")));
    let failed = cache.latest();
    assert_eq!(failed.state, State::Stale);
    assert_eq!(failed.devices, before.devices);
    assert!(failed.age_ms.unwrap() >= 4000);
    cache.update(parse(&report(json!([])), &RandomState::new()));
    assert_eq!(cache.latest().state, State::Ready);
    assert!(cache.latest().devices.is_empty());
  }

  fn command(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command
      .args(["-c", script])
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .process_group(0);
    command
  }

  #[test]
  fn child_reports_are_bounded_fail_closed_and_cancel_promptly() {
    let stop = (Mutex::new(false), Condvar::new());
    assert_eq!(
      capture(command("printf report"), &stop, Duration::from_secs(1)).unwrap(),
      b"report"
    );
    assert!(capture(command("exit 1"), &stop, Duration::from_secs(1)).is_err());
    let started = Instant::now();
    assert!(capture(command("sleep 30"), &stop, Duration::from_millis(60)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
      capture(command("/bin/dd if=/dev/zero bs=65536 count=17"), &stop, Duration::from_secs(4))
        .is_err()
    );
    *stop.0.lock().unwrap() = true;
    assert!(capture(command("sleep 30"), &stop, Duration::from_secs(4)).is_err());
  }

  #[test]
  fn worker_read_is_nonblocking_and_drop_joins_and_cancels_active_child() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let worker = Worker::spawn(move |stop, _| {
      observed.fetch_add(1, Ordering::SeqCst);
      capture(command("sleep 30"), stop, TIMEOUT).map(|_| Snapshot::default())
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while calls.load(Ordering::SeqCst) == 0 {
      assert!(Instant::now() < deadline);
      thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    assert_eq!(worker.reader().latest().state, State::Warming);
    assert!(started.elapsed() < Duration::from_millis(100));
    drop(worker);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn old_sample_details_without_bluetooth_remain_compatible() {
    let old = serde_json::to_value(crate::serve::SampleDetails::default()).unwrap();
    let mut old = old.as_object().unwrap().clone();
    old.remove("bluetooth");
    assert!(
      serde_json::from_value::<crate::serve::SampleDetails>(Value::Object(old))
        .unwrap()
        .bluetooth
        .is_none()
    );
  }

  #[test]
  #[ignore = "reads actual connected-device report on the owner's Mac"]
  fn native_connected_device_report_is_rootless_and_worker_stops() {
    let worker = Worker::start().unwrap();
    let reader = worker.reader();
    let started = Instant::now();
    let snapshot = loop {
      let snapshot = reader.latest();
      if snapshot.state != State::Warming {
        break snapshot;
      }
      assert!(started.elapsed() < Duration::from_secs(6));
      thread::sleep(Duration::from_millis(20));
    };
    println!(
      "native Bluetooth state={:?} controller={:?} connected={} battery_devices={} components={:?} elapsed_ms={}",
      snapshot.state,
      snapshot.controller,
      snapshot.devices.len(),
      snapshot.devices.iter().filter(|device| !device.batteries.is_empty()).count(),
      snapshot
        .devices
        .iter()
        .map(|device| (
          device.kind.as_str(),
          device
            .batteries
            .iter()
            .map(|battery| (battery.component.as_str(), battery.percent))
            .collect::<Vec<_>>()
        ))
        .collect::<Vec<_>>(),
      started.elapsed().as_millis()
    );
    assert_eq!(snapshot.state, State::Ready);
    drop(worker);
    assert!(started.elapsed() < Duration::from_secs(7));
  }
}
