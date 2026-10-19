//! App-only NStat accounting. `nettop -P` reports cumulative TCP/UDP process summaries,
//! including loopback and proxy/VPN sockets. These are not interface or Internet totals.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::proc_groups::{self, GroupKind, Identity, Ownership};

const INTERVAL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(6);
const MAX_OUTPUT: usize = 1024 * 1024;
const MAX_PROCESSES: usize = 4096;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Member {
  pub pid: i32,
  pub instance: String,
  pub name: String,
  pub rx_bytes_per_sec: Option<f64>,
  pub tx_bytes_per_sec: Option<f64>,
  pub rx_session_bytes: u64,
  pub tx_session_bytes: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Group {
  pub id: String,
  pub name: String,
  pub kind: GroupKind,
  pub members: Vec<Member>,
  pub rx_bytes_per_sec: Option<f64>,
  pub tx_bytes_per_sec: Option<f64>,
  pub rx_session_bytes: u64,
  pub tx_session_bytes: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Snapshot {
  /// warming, ready, or unavailable; failed observations retain their original age.
  pub state: String,
  pub sequence: u64,
  #[serde(with = "crate::source::instant")]
  pub sampled: Instant,
  pub groups: Vec<Group>,
  pub observed_processes: usize,
  pub excluded_processes: usize,
}

impl Default for Snapshot {
  fn default() -> Self {
    Self {
      state: "warming".into(),
      sequence: 0,
      sampled: Instant::now(),
      groups: Vec::new(),
      observed_processes: 0,
      excluded_processes: 0,
    }
  }
}

impl Snapshot {
  pub(crate) fn value(&self) -> serde_json::Value {
    serde_json::json!({"state": self.state, "sequence": self.sequence,
      "age_ms": self.sampled.elapsed().as_millis().min(u64::MAX as u128) as u64,
      "interval_ms": INTERVAL.as_millis(), "groups": self.groups,
      "observed_processes": self.observed_processes, "excluded_processes": self.excluded_processes})
  }
}

#[derive(Clone, Debug)]
struct NativeProcess {
  identity: Identity,
  uid: u32,
  path: String,
  coalition: Option<proc_groups::Coalition>,
}

#[derive(Clone, Debug)]
struct NativeSnapshot {
  verified: HashMap<i32, NativeProcess>,
  /// None means enumeration was unavailable/truncated, not that every PID exited.
  live: Option<HashSet<i32>>,
  /// Fresh unreadable metadata may hide competing owners; never use these as owner seeds.
  blockers: HashSet<(Option<u32>, proc_groups::Coalition)>,
}

impl std::ops::Deref for NativeSnapshot {
  type Target = HashMap<i32, NativeProcess>;
  fn deref(&self) -> &Self::Target {
    &self.verified
  }
}

#[cfg(test)]
impl std::ops::DerefMut for NativeSnapshot {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.verified
  }
}

impl From<HashMap<i32, NativeProcess>> for NativeSnapshot {
  fn from(verified: HashMap<i32, NativeProcess>) -> Self {
    Self { live: Some(verified.keys().copied().collect()), verified, blockers: HashSet::new() }
  }
}

fn processes() -> NativeSnapshot {
  let mut pids = Vec::new();
  let complete = crate::procs::list_pids(&mut pids);
  pids.retain(|pid| *pid > 0);
  let live = (complete && !pids.is_empty() && pids.len() <= MAX_PROCESSES)
    .then(|| pids.iter().copied().collect());
  let mut verified = HashMap::new();
  let mut blockers = HashSet::new();
  for pid in pids.into_iter().take(MAX_PROCESSES) {
    let before = proc_groups::identity(pid);
    let uid = crate::procs::real_uid(pid);
    let path = crate::procs::exe_path(pid);
    let coalition = proc_groups::coalition(pid);
    let after = proc_groups::identity(pid);
    if let (Some(identity), Some(uid), Some(path)) = (before, uid, path)
      && Some(identity) == after
    {
      verified.insert(pid, NativeProcess { identity, uid, path, coalition });
    } else if let Some(coalition) = coalition {
      blockers.insert((uid, coalition));
    }
  }
  NativeSnapshot { verified, live, blockers }
}

/// Each CSV block is absolute (there is no `-d`). Read counters only;
/// never trust a truncated process label as an application ownership or PID lifetime.
fn header(line: &[u8]) -> Option<bool> {
  let line = std::str::from_utf8(line).ok()?.trim_end_matches(['\r', '\n']);
  let columns: Vec<_> = line.strip_suffix(',').unwrap_or(line).split(',').collect();
  let columns = columns.strip_prefix(&["time"]).unwrap_or(&columns);
  if columns.len() != 3
    || !columns[0].is_empty()
    || !columns.contains(&"bytes_in")
    || !columns.contains(&"bytes_out")
  {
    return None;
  }
  Some(columns[1] == "bytes_in")
}

fn parse(output: &[u8]) -> io::Result<HashMap<i32, (u64, u64)>> {
  let text = std::str::from_utf8(output).map_err(|_| invalid("non-UTF8 nettop output"))?;
  let mut lines = text.lines();
  let first = lines.next().ok_or_else(|| invalid("missing nettop header"))?;
  let rx_first = header(first.as_bytes()).ok_or_else(|| invalid("unknown nettop columns"))?;
  let mut result = HashMap::new();
  for line in lines {
    if line.len() > 8192 {
      return Err(invalid("oversized nettop row"));
    }
    let mut fields = line.trim_end_matches(',').rsplitn(3, ',');
    let last = fields.next().and_then(|value| value.parse::<u64>().ok());
    let first = fields.next().and_then(|value| value.parse::<u64>().ok());
    let label = fields.next().ok_or_else(|| invalid("malformed nettop row"))?.trim_matches('"');
    let pid =
      label.rsplit_once('.').and_then(|(_, pid)| pid.parse::<i32>().ok()).filter(|pid| *pid > 0);
    let (Some(pid), Some(first), Some(last)) = (pid, first, last) else {
      return Err(invalid("invalid nettop counter"));
    };
    let counters = if rx_first { (first, last) } else { (last, first) };
    if result.insert(pid, counters).is_some() || result.len() > MAX_PROCESSES {
      return Err(invalid("duplicate or excessive nettop processes"));
    }
  }
  Ok(result)
}

fn invalid(message: &str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Child ownership stays on the worker stack, including pipe errors and cancellation.
struct Running {
  process: Child,
  terminal: Option<File>,
}
impl Drop for Running {
  fn drop(&mut self) {
    // Darwin can keep a killed controlling-terminal owner EXITING until its master closes.
    drop(self.terminal.take());
    let _ = self.process.kill();
    let _ = self.process.wait();
  }
}

/// CSV stdout needs a private PTY: native nettop leaves a pipe buffered for many samples.
/// It remains in explicit logging mode, with no user terminal, input or terminal settings.
fn observe(
  command: &mut Command,
  stopped: &AtomicBool,
  timeout: Duration,
  mut publish: impl FnMut(HashMap<i32, (u64, u64)>, &NativeSnapshot, &NativeSnapshot, Instant),
) -> io::Result<()> {
  let (mut master, mut slave) = (-1, -1);
  if unsafe {
    libc::openpty(
      &mut master,
      &mut slave,
      std::ptr::null_mut(),
      std::ptr::null_mut(),
      std::ptr::null_mut(),
    )
  } < 0
  {
    return Err(io::Error::last_os_error());
  }
  let stdout = unsafe { File::from_raw_fd(master) };
  let slave = unsafe { File::from_raw_fd(slave) };
  for fd in [stdout.as_raw_fd(), slave.as_raw_fd()] {
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
      return Err(io::Error::last_os_error());
    }
  }
  let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
  if flags < 0
    || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
  {
    return Err(io::Error::last_os_error());
  }
  let mut before = processes();
  let mut next_before = before.clone();
  // An independent private controlling terminal ties abnormal owner death to SIGHUP.
  // Only async-signal-safe syscalls run between fork and exec.
  unsafe {
    command.pre_exec(|| {
      let mut action: libc::sigaction = std::mem::zeroed();
      action.sa_sigaction = libc::SIG_DFL;
      libc::sigemptyset(&mut action.sa_mask);
      if libc::sigaction(libc::SIGHUP, &action, std::ptr::null_mut()) < 0 {
        return Err(io::Error::last_os_error());
      }
      if libc::setsid() < 0
        || libc::ioctl(libc::STDOUT_FILENO, libc::TIOCSCTTY as libc::c_ulong, 0) < 0
      {
        return Err(io::Error::last_os_error());
      }
      Ok(())
    });
  }
  // nettop watches stdin even in logging mode: /dev/null's permanent EOF causes a busy loop.
  // The unread private slave has no user input and stays idle between native observations.
  let process = command
    .stdin(Stdio::from(slave.try_clone()?))
    .stdout(Stdio::from(slave))
    .stderr(Stdio::null())
    .spawn()?;
  let mut child = Running { process, terminal: Some(stdout) };
  let mut deadline = Instant::now() + timeout;
  let mut sampled = Instant::now();
  let mut block = Vec::new();
  let mut pending = Vec::new();
  let mut buf = [0; 8192];
  loop {
    if stopped.load(Ordering::Acquire) {
      return Ok(());
    }
    if Instant::now() >= deadline {
      return Err(io::Error::new(io::ErrorKind::TimedOut, "nettop timed out"));
    }
    for _ in 0..16 {
      let count = match child.terminal.as_mut().unwrap().read(&mut buf) {
        Ok(0) => return Err(io::Error::other("nettop output closed")),
        Ok(count) => count,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
        Err(error) => return Err(error),
      };
      pending.extend_from_slice(&buf[..count]);
      while let Some(end) = pending.iter().position(|&byte| byte == b'\n') {
        let line: Vec<_> = pending.drain(..=end).collect();
        if header(&line).is_some() {
          let header_at = Instant::now();
          let after = processes();
          if !block.is_empty() {
            publish(parse(&block)?, &before, &after, sampled);
          }
          before = std::mem::replace(&mut next_before, after);
          block.clear();
          sampled = header_at;
          deadline = sampled + timeout;
        }
        block.extend(line);
        if block.len() > MAX_OUTPUT {
          return Err(invalid("nettop output limit"));
        }
      }
      if pending.len() > 8192 {
        return Err(invalid("nettop row limit"));
      }
    }
    if child.process.try_wait()?.is_some() {
      return Err(io::Error::other("nettop exited"));
    }
    thread::sleep(Duration::from_millis(20));
  }
}

#[derive(Clone)]
struct Previous {
  identity: Identity,
  uid: u32,
  counters: (u64, u64),
  sampled: Instant,
  valid: bool,
  instance: String,
  totals: (u64, u64),
}

struct Accounting {
  ownership: Ownership,
  known: HashMap<i32, (NativeProcess, Instant)>,
  previous: HashMap<i32, Previous>,
  lifetimes: HashMap<i32, (Identity, u32)>,
  quarantined: HashSet<i32>,
  totals: HashMap<String, (u64, u64)>,
  snapshot: Snapshot,
}
impl Accounting {
  fn new() -> Self {
    Self {
      ownership: Ownership::new(),
      known: HashMap::new(),
      previous: HashMap::new(),
      lifetimes: HashMap::new(),
      quarantined: HashSet::new(),
      totals: HashMap::new(),
      snapshot: Snapshot::default(),
    }
  }
  fn failed(&mut self) -> Snapshot {
    self.previous.values_mut().for_each(|previous| previous.valid = false);
    self.lifetimes.clear();
    self.quarantined.clear();
    self.snapshot.state = "unavailable".into();
    self.snapshot.clone()
  }
  fn ingest(
    &mut self,
    counters: HashMap<i32, (u64, u64)>,
    before: &NativeSnapshot,
    after: &NativeSnapshot,
    sampled: Instant,
  ) -> Snapshot {
    // NStat may retain closed sockets. Never reassign an uninterrupted old row to a
    // proven reused PID/exec or changed real UID; a row boundary/new observer clears it.
    self.lifetimes.retain(|pid, _| counters.contains_key(pid));
    self.quarantined.retain(|pid| counters.contains_key(pid));
    for &pid in counters.keys() {
      if let (Some(before), Some(after)) = (before.get(&pid), after.get(&pid))
        && (before.identity != after.identity || before.uid != after.uid)
      {
        self.quarantined.insert(pid);
      }
      if self.lifetimes.get(&pid).is_some_and(|old| {
        [before.get(&pid), after.get(&pid)]
          .into_iter()
          .flatten()
          .any(|current| *old != (current.identity, current.uid))
      }) {
        self.quarantined.insert(pid);
      }
    }
    let mut stable: Vec<_> = before
      .iter()
      .filter(|(pid, process)| {
        after.get(pid).is_some_and(|after| {
          after.identity == process.identity
            && after.uid == process.uid
            && after.path == process.path
            && after.coalition == process.coalition
        })
      })
      .collect();
    stable.sort_by_key(|(pid, _)| **pid);
    stable.truncate(MAX_PROCESSES);
    let fresh: HashSet<_> = stable.iter().map(|(pid, _)| **pid).collect();
    self.known.retain(|pid, (old, _)| {
      ![before.live.as_ref(), after.live.as_ref()]
        .into_iter()
        .flatten()
        .any(|live| !live.contains(pid))
        && ![before.get(pid), after.get(pid)]
          .into_iter()
          .flatten()
          .any(|current| current.identity != old.identity || current.uid != old.uid)
    });
    for &(pid, process) in &stable {
      self.known.insert(*pid, (process.clone(), sampled));
    }
    // Prefer fresh evidence, evicting oldest uncertain identities first under churn.
    let mut uncertain: Vec<_> = self
      .known
      .iter()
      .filter(|(pid, _)| !fresh.contains(pid))
      .map(|(&pid, (_, seen))| (pid, *seen))
      .collect();
    uncertain.sort_by_key(|(pid, seen)| (*seen, *pid));
    for (pid, _) in uncertain.iter().take(self.known.len().saturating_sub(MAX_PROCESSES)) {
      self.known.remove(pid);
    }
    let retained: Vec<_> = self
      .known
      .iter()
      .filter(|(pid, _)| !fresh.contains(pid))
      .map(|(_, (process, _))| (process.uid, process.path.as_str()))
      .collect();
    let mut entries: Vec<_> = stable
      .iter()
      .map(|(_, process)| (process.uid, Some(process.path.as_str()), process.coalition))
      .collect();
    // Uncertain historical rows may invalidate coalition evidence, never seed an owner.
    entries.extend(
      self
        .known
        .iter()
        .filter(|(pid, _)| !fresh.contains(pid))
        .map(|(_, (process, _))| (process.uid, None, process.coalition)),
    );
    // An unknown UID blocks every candidate UID in that coalition. A truncated PID
    // list cannot establish a unique coalition owner, even among readable rows.
    let blockers: HashSet<_> = before.blockers.union(&after.blockers).copied().collect();
    let unknown_coalitions: HashSet<_> =
      blockers.iter().filter_map(|(uid, coalition)| uid.is_none().then_some(*coalition)).collect();
    let uncertain: HashSet<_> = entries
      .iter()
      .filter_map(|&(uid, _, coalition)| {
        coalition
          .filter(|coalition| {
            before.live.is_none() || after.live.is_none() || unknown_coalitions.contains(coalition)
          })
          .map(|coalition| (uid, coalition))
      })
      .chain(blockers.into_iter().filter_map(|(uid, coalition)| uid.map(|uid| (uid, coalition))))
      .collect();
    entries.extend(uncertain.into_iter().map(|(uid, coalition)| (uid, None, Some(coalition))));
    let apps = self.ownership.applications_retaining(&entries, &retained);
    let live_apps: HashSet<_> = self.ownership.application_ids().collect();
    self.totals.retain(|id, _| live_apps.contains(id.as_str()));
    let observed = counters.len();
    let mut groups = HashMap::<String, Group>::new();
    let mut previous = HashMap::new();
    for ((&pid, process), app) in stable.into_iter().zip(apps) {
      let Some(&current) = counters.get(&pid) else {
        continue;
      };
      if self.quarantined.contains(&pid) {
        continue;
      }
      self.lifetimes.insert(pid, (process.identity, process.uid));
      let old = self
        .previous
        .get(&pid)
        .filter(|old| old.identity == process.identity && old.uid == process.uid);
      let instance = old.map(|old| old.instance.clone()).unwrap_or_else(|| self.ownership.fresh());
      let name = process
        .path
        .rsplit('/')
        .next()
        .unwrap_or("Process")
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect::<String>();
      let (id, title, kind) = app
        .map(|app| {
          (
            app.id,
            app.name.chars().filter(|c| !c.is_control()).take(100).collect(),
            GroupKind::Application,
          )
        })
        .unwrap_or_else(|| (instance.clone(), name.clone(), GroupKind::Process));
      let delta = old
        .filter(|old| {
          old.valid
            && current.0 >= old.counters.0
            && current.1 >= old.counters.1
            && sampled > old.sampled
        })
        .map(|old| {
          (
            current.0 - old.counters.0,
            current.1 - old.counters.1,
            sampled.duration_since(old.sampled).as_secs_f64(),
          )
        });
      let totals = old.map_or((0, 0), |old| old.totals);
      let totals = (
        totals.0.saturating_add(delta.map_or(0, |delta| delta.0)),
        totals.1.saturating_add(delta.map_or(0, |delta| delta.1)),
      );
      // Application totals outlive network-active members while a confirmed app remains live.
      // Non-application rows have one PID lifetime, already accumulated in Previous.
      let sum = self.totals.entry(id.clone()).or_default();
      if kind == GroupKind::Process {
        *sum = totals;
      } else {
        sum.0 = sum.0.saturating_add(delta.map_or(0, |delta| delta.0));
        sum.1 = sum.1.saturating_add(delta.map_or(0, |delta| delta.1));
      }
      let member = Member {
        pid,
        instance: instance.clone(),
        name,
        rx_bytes_per_sec: delta.map(|delta| delta.0 as f64 / delta.2),
        tx_bytes_per_sec: delta.map(|delta| delta.1 as f64 / delta.2),
        rx_session_bytes: totals.0,
        tx_session_bytes: totals.1,
      };
      let group = groups.entry(id.clone()).or_insert_with(|| Group {
        id,
        name: title,
        kind,
        members: Vec::new(),
        rx_bytes_per_sec: Some(0.0),
        tx_bytes_per_sec: Some(0.0),
        rx_session_bytes: 0,
        tx_session_bytes: 0,
      });
      group.rx_bytes_per_sec =
        group.rx_bytes_per_sec.zip(member.rx_bytes_per_sec).map(|(a, b)| a + b);
      group.tx_bytes_per_sec =
        group.tx_bytes_per_sec.zip(member.tx_bytes_per_sec).map(|(a, b)| a + b);
      group.rx_session_bytes = sum.0;
      group.tx_session_bytes = sum.1;
      group.members.push(member);
      previous.insert(
        pid,
        Previous {
          identity: process.identity,
          uid: process.uid,
          counters: current,
          sampled,
          valid: true,
          instance,
          totals,
        },
      );
    }
    // A missing NStat row is not a process exit. Keep the same verified lifetime's
    // identity and observed totals, but never count traffic across the missing window.
    for (pid, mut old) in self.previous.drain() {
      if !previous.contains_key(&pid)
        && self
          .known
          .get(&pid)
          .is_some_and(|(native, _)| native.identity == old.identity && native.uid == old.uid)
      {
        old.valid = false;
        previous.insert(pid, old);
      }
    }
    self.previous = previous;
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by(|a, b| a.id.cmp(&b.id));
    let excluded =
      observed.saturating_sub(groups.iter().map(|group| group.members.len()).sum::<usize>());
    self.snapshot = Snapshot {
      state: if observed > 0 && groups.is_empty()
        || groups.iter().any(|group| group.rx_bytes_per_sec.is_none())
      {
        "warming"
      } else {
        "ready"
      }
      .into(),
      sequence: self.snapshot.sequence.saturating_add(1),
      sampled,
      groups,
      observed_processes: observed,
      excluded_processes: excluded,
    };
    self.snapshot.clone()
  }
}

/// Kept by `source::run`, so every return path cancels, kills/reaps and joins the worker.
/// Blocking NStat collection never runs on the system sampling or control threads.
pub(crate) struct Worker {
  stopped: Arc<AtomicBool>,
  join: Option<JoinHandle<()>>,
}
impl Worker {
  pub(crate) fn start(mut publish: impl FnMut(Snapshot) + Send + 'static) -> io::Result<Self> {
    let stopped = Arc::new(AtomicBool::new(false));
    let cancel = Arc::clone(&stopped);
    let join = thread::Builder::new().name("app-network".into()).spawn(move || {
      let mut accounting = Accounting::new();
      while !cancel.load(Ordering::Acquire) {
        let started = Instant::now();
        let result = observe(
          Command::new("/usr/bin/nettop").args([
            "-P",
            "-L",
            "0",
            "-s",
            "2",
            "-n",
            "-x",
            "-J",
            "bytes_in,bytes_out",
          ]),
          &cancel,
          TIMEOUT,
          |counters, before, after, sampled| {
            publish(accounting.ingest(counters, before, after, sampled));
          },
        );
        if cancel.load(Ordering::Acquire) {
          break;
        }
        if result.is_err() {
          publish(accounting.failed());
        }
        while started.elapsed() < INTERVAL && !cancel.load(Ordering::Acquire) {
          thread::sleep(Duration::from_millis(20));
        }
      }
    })?;
    Ok(Self { stopped, join: Some(join) })
  }
}
impl Drop for Worker {
  fn drop(&mut self) {
    self.stopped.store(true, Ordering::Release);
    if let Some(join) = self.join.take() {
      let _ = join.join();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn native(unique: u64, path: &str) -> NativeProcess {
    NativeProcess {
      identity: Identity { unique, version: 1 },
      uid: 501,
      path: path.into(),
      coalition: Some([90, 91]),
    }
  }
  fn table() -> NativeSnapshot {
    HashMap::from([
      (10, native(101, "/Applications/Browser.app/Contents/MacOS/Browser")),
      (
        11,
        native(
          102,
          "/Applications/Browser.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper",
        ),
      ),
    ])
    .into()
  }
  fn account() -> Accounting {
    let mut result = Accounting::new();
    result.ownership.set_default_for_test(Some([1, 2]));
    result
  }

  #[test]
  fn absolute_csv_supports_native_headers_dotted_comma_and_quoted_labels() {
    let raw = b",bytes_in,bytes_out,\r\nio.tailscale.ip.847,2561337,31186108,\r\nname,with,commas.32,0,8,\r\n\"quoted,name.52\",19,0,\r\n";
    assert!(header(b",bytes_tool.42,100,200,\n").is_none());
    assert_eq!(header(b",bytes_out,bytes_in,\r\n"), Some(false));
    let parsed = parse(raw).unwrap();
    assert_eq!(parsed[&847], (2561337, 31186108));
    assert_eq!(parsed[&32], (0, 8));
    assert_eq!(parsed[&52], (19, 0));
    assert_eq!(parse(b",bytes_out,bytes_in,\nx.2,9,3,\n").unwrap()[&2], (3, 9));
    assert_eq!(
      parse(b"time,,bytes_in,bytes_out,\n23:48:39.549138,Python.84370,2097152,0,\n").unwrap()
        [&84370],
      (2097152, 0)
    );
    assert!(parse(b",bytes_in,bytes_out,\n").unwrap().is_empty());
  }

  #[test]
  fn malformed_duplicate_overflow_and_unsupported_output_never_become_zero() {
    for bad in [
      b"".as_slice(),
      b"nettop: NStatManagerCreate failed",
      b",bytes_in,extra,\nx.3,1,2,\n",
      b",bytes_in,bytes_out,\nx.3,-1,2,\n",
      b",bytes_in,bytes_out,\nx.3,18446744073709551616,2,\n",
      b",bytes_in,bytes_out,\nx.3,1,2,\nx.3,1,2,\n",
      b",bytes_in,bytes_out,\nx.0,1,2,\n",
      b",bytes_in,bytes_out,\nx.3,1,\n",
    ] {
      assert!(parse(bad).is_err(), "{bad:?}");
    }
  }

  #[test]
  fn application_totals_survive_exited_helpers_new_pids_and_counter_resets() {
    let mut reader = account();
    let processes = table();
    let start = Instant::now();
    let first = reader.ingest(
      HashMap::from([(10, (1000, 100)), (11, (3000, 300))]),
      &processes,
      &processes,
      start,
    );
    assert_eq!(first.groups.len(), 1);
    assert_eq!(first.groups[0].members.len(), 2);
    assert!(first.groups[0].rx_bytes_per_sec.is_none());
    assert_eq!(first.groups[0].rx_session_bytes, 0);
    let second = reader.ingest(
      HashMap::from([(10, (1200, 140)), (11, (3400, 360))]),
      &processes,
      &processes,
      start + INTERVAL,
    );
    let group = &second.groups[0];
    assert_eq!(group.rx_bytes_per_sec, Some(300.0));
    assert_eq!(group.tx_bytes_per_sec, Some(50.0));
    assert_eq!(group.rx_session_bytes, 600);
    let mut after_exit = processes.clone();
    after_exit.remove(&11);
    after_exit.live.as_mut().unwrap().remove(&11);
    let exited = reader.ingest(
      HashMap::from([(10, (1300, 150))]),
      &after_exit,
      &after_exit,
      start + INTERVAL * 2,
    );
    assert_eq!(exited.groups[0].rx_session_bytes, 700);
    assert_eq!(exited.groups[0].members.len(), 1);
    after_exit.live.as_mut().unwrap().insert(12);
    after_exit.insert(
      12,
      native(103, "/Applications/Browser.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"),
    );
    let restarted = reader.ingest(
      HashMap::from([(10, (1400, 160)), (12, (9000, 9000))]),
      &after_exit,
      &after_exit,
      start + INTERVAL * 3,
    );
    assert_eq!(restarted.groups[0].rx_session_bytes, 800);
    assert!(restarted.groups[0].rx_bytes_per_sec.is_none());
    assert_eq!(restarted.groups[0].members[1].rx_session_bytes, 0);
    let reset = reader.ingest(
      HashMap::from([(10, (1, 1)), (12, (9010, 9020))]),
      &after_exit,
      &after_exit,
      start + INTERVAL * 4,
    );
    assert_eq!(reset.groups[0].rx_session_bytes, 810);
    assert!(reset.groups[0].rx_bytes_per_sec.is_none());
  }

  #[test]
  fn lifetime_bracketing_and_missing_ownership_never_attribute_a_reused_pid() {
    let mut reader = account();
    let before = table();
    let start = Instant::now();
    reader.ingest(HashMap::from([(10, (100, 100))]), &before, &before, start);
    let mut after = before.clone();
    after.get_mut(&10).unwrap().identity.unique += 1;
    let reused = reader.ingest(
      HashMap::from([(10, (999_999, 999_999)), (77, (200, 200))]),
      &before,
      &after,
      start + INTERVAL,
    );
    assert!(reused.groups.is_empty());
    assert_eq!(reused.excluded_processes, 2);
    let fresh = reader.ingest(
      HashMap::from([(10, (1_000_000, 1_000_000))]),
      &after,
      &after,
      start + INTERVAL * 2,
    );
    assert!(fresh.groups.is_empty());
    reader.ingest(HashMap::new(), &after, &after, start + INTERVAL * 3);
    let fresh = reader.ingest(
      HashMap::from([(10, (1_000_000, 1_000_000))]),
      &after,
      &after,
      start + INTERVAL * 4,
    );
    assert!(fresh.groups[0].rx_bytes_per_sec.is_none());
    assert_eq!(fresh.groups[0].rx_session_bytes, 0);
    let mut exec = after.clone();
    exec.get_mut(&10).unwrap().identity.version += 1;
    let changed = reader.ingest(
      HashMap::from([(10, (1_000_010, 1_000_010))]),
      &after,
      &exec,
      start + INTERVAL * 5,
    );
    assert!(changed.groups.is_empty());
    assert!(serde_json::to_string(&fresh.value()).unwrap().find("/Applications").is_none());
  }

  #[test]
  fn unowned_process_totals_accumulate_and_failures_preserve_original_age_without_rates() {
    let mut reader = account();
    let native: NativeSnapshot = HashMap::from([(10, native(1, "/usr/bin/curl"))]).into();
    let time = Instant::now() - Duration::from_secs(20);
    reader.ingest(HashMap::from([(10, (100, 10))]), &native, &native, time);
    reader.ingest(HashMap::from([(10, (200, 20))]), &native, &native, time + INTERVAL);
    let third =
      reader.ingest(HashMap::from([(10, (250, 30))]), &native, &native, time + INTERVAL * 2);
    assert_eq!(third.groups[0].kind, GroupKind::Process);
    assert_eq!(third.groups[0].rx_session_bytes, 150);
    assert_eq!(third.groups[0].members[0].rx_session_bytes, 150);
    let failed = reader.failed();
    assert_eq!(failed.state, "unavailable");
    assert_eq!(failed.sampled, third.sampled);
    assert_eq!(failed.sequence, third.sequence);
    assert!(failed.value()["age_ms"].as_u64().unwrap() >= 16000);
    let recovered =
      reader.ingest(HashMap::from([(10, (9000, 9000))]), &native, &native, time + INTERVAL * 3);
    assert!(recovered.groups[0].rx_bytes_per_sec.is_none());
    assert_eq!(recovered.groups[0].rx_session_bytes, 150);
  }

  #[test]
  fn same_live_pid_keeps_its_instance_and_session_totals_through_missing_rows() {
    let mut reader = account();
    let native: NativeSnapshot = HashMap::from([(10, native(1, "/usr/bin/curl"))]).into();
    let time = Instant::now();
    reader.ingest(HashMap::from([(10, (100, 10))]), &native, &native, time);
    let measured =
      reader.ingest(HashMap::from([(10, (250, 30))]), &native, &native, time + INTERVAL);
    let instance = measured.groups[0].members[0].instance.clone();
    assert_eq!(measured.groups[0].rx_session_bytes, 150);
    let missing = reader.ingest(HashMap::new(), &native, &native, time + INTERVAL * 2);
    assert!(missing.groups.is_empty());
    assert_eq!(reader.previous.len(), 1);
    let recovered =
      reader.ingest(HashMap::from([(10, (900, 100))]), &native, &native, time + INTERVAL * 3);
    assert_eq!(recovered.groups[0].members[0].instance, instance);
    assert_eq!(recovered.groups[0].rx_session_bytes, 150);
    assert_eq!(recovered.groups[0].members[0].rx_session_bytes, 150);
    assert!(recovered.groups[0].rx_bytes_per_sec.is_none());
    let next =
      reader.ingest(HashMap::from([(10, (1000, 120))]), &native, &native, time + INTERVAL * 4);
    assert_eq!(next.groups[0].rx_session_bytes, 250);
    assert_eq!(next.groups[0].members[0].instance, instance);
    reader.ingest(
      HashMap::new(),
      &HashMap::new().into(),
      &HashMap::new().into(),
      time + INTERVAL * 5,
    );
    assert!(reader.previous.is_empty());
  }

  #[test]
  fn incomplete_metadata_preserves_confirmed_lifetime_totals_without_publishing_members() {
    for path in ["/usr/bin/curl", "/Applications/Browser.app/Contents/MacOS/Browser"] {
      let mut reader = account();
      let native: NativeSnapshot = HashMap::from([(10, native(1, path))]).into();
      let time = Instant::now();
      reader.ingest(HashMap::from([(10, (100, 10))]), &native, &native, time);
      let measured =
        reader.ingest(HashMap::from([(10, (250, 30))]), &native, &native, time + INTERVAL);
      let instance = measured.groups[0].members[0].instance.clone();
      let group_id = measured.groups[0].id.clone();
      let mut unknown = native.clone();
      unknown.verified.clear(); // Complete PID enumeration still proves the process is live.
      for (index, (before, after)) in
        [(&native, &unknown), (&unknown, &unknown), (&unknown, &native)].into_iter().enumerate()
      {
        let hidden = reader.ingest(
          HashMap::from([(10, (800, 80))]),
          before,
          after,
          time + INTERVAL * (index as u32 + 2),
        );
        assert!(hidden.groups.is_empty());
        assert_eq!(hidden.state, "warming");
        assert_eq!(hidden.excluded_processes, 1);
        assert_eq!(reader.previous[&10].instance, instance);
        assert!(!reader.previous[&10].valid);
      }
      let recovered =
        reader.ingest(HashMap::from([(10, (900, 100))]), &native, &native, time + INTERVAL * 5);
      assert_eq!(recovered.groups[0].id, group_id);
      assert_eq!(recovered.groups[0].members[0].instance, instance);
      assert_eq!(recovered.groups[0].members[0].rx_session_bytes, 150);
      assert_eq!(recovered.groups[0].rx_session_bytes, 150);
      assert!(recovered.groups[0].rx_bytes_per_sec.is_none());
      let next =
        reader.ingest(HashMap::from([(10, (1000, 120))]), &native, &native, time + INTERVAL * 6);
      assert_eq!(next.groups[0].rx_session_bytes, 250);
      reader.ingest(HashMap::new(), &native, &HashMap::new().into(), time + INTERVAL * 7);
      assert!(reader.previous.is_empty() && reader.known.is_empty());
    }
  }

  #[test]
  fn complete_before_enumeration_retires_history_when_after_is_unavailable() {
    let mut reader = account();
    let old: NativeSnapshot =
      HashMap::from([(10, native(1, "/Applications/Browser.app/Contents/MacOS/Browser"))]).into();
    let time = Instant::now();
    reader.ingest(HashMap::from([(10, (100, 10))]), &old, &old, time);
    let measured = reader.ingest(HashMap::from([(10, (250, 30))]), &old, &old, time + INTERVAL);
    let old_group = measured.groups[0].id.clone();
    let old_instance = measured.groups[0].members[0].instance.clone();
    let before: NativeSnapshot = HashMap::from([(20, native(2, "/usr/bin/curl"))]).into();
    let unknown = NativeSnapshot { verified: HashMap::new(), live: None, blockers: HashSet::new() };
    reader.ingest(HashMap::from([(10, (900, 100))]), &before, &unknown, time + INTERVAL * 2);
    assert!(reader.known.is_empty() && reader.previous.is_empty());
    assert!(!reader.totals.contains_key(&old_group));
    let recovered =
      reader.ingest(HashMap::from([(10, (1000, 120))]), &old, &old, time + INTERVAL * 3);
    assert_ne!(recovered.groups[0].id, old_group);
    assert_ne!(recovered.groups[0].members[0].instance, old_instance);
    assert_eq!(recovered.groups[0].rx_session_bytes, 0);
    assert!(recovered.groups[0].rx_bytes_per_sec.is_none());
  }

  #[test]
  fn first_seen_unresolved_metadata_blocks_coalition_ownership_without_seeding_it() {
    for uid in [Some(501), None] {
      let mut reader = account();
      let mut snapshot: NativeSnapshot = HashMap::from([
        (10, native(1, "/Applications/Browser.app/Contents/MacOS/Browser")),
        (11, native(2, "/System/Library/Frameworks/WebKit.framework/XPCServices/web.xpc/Contents/MacOS/service")),
      ]).into();
      snapshot.blockers.insert((uid, [90, 91]));
      snapshot.live.as_mut().unwrap().insert(12); // First-seen competing owner has unreadable metadata.
      let time = Instant::now();
      let counters = HashMap::from([(10, (100, 10)), (11, (200, 20)), (12, (300, 30))]);
      let blocked = reader.ingest(counters.clone(), &snapshot, &snapshot, time);
      assert_eq!(blocked.groups.len(), 2);
      assert_eq!(blocked.excluded_processes, 1);
      assert!(blocked.groups.iter().all(|group| group.members.len() == 1));
      assert!(
        blocked
          .groups
          .iter()
          .any(|group| group.kind == GroupKind::Process && group.members[0].pid == 11)
      );
      snapshot.blockers.clear();
      snapshot.live = None; // An incomplete enumeration also cannot establish owner uniqueness.
      let partial = reader.ingest(counters.clone(), &snapshot, &snapshot, time + INTERVAL);
      assert_eq!(partial.groups.len(), 2);
      snapshot.live = Some(snapshot.verified.keys().copied().collect());
      let resolved = reader.ingest(counters, &snapshot, &snapshot, time + INTERVAL * 2);
      assert_eq!(resolved.groups.len(), 1);
      assert_eq!(resolved.groups[0].kind, GroupKind::Application);
      assert_eq!(resolved.groups[0].members.len(), 2);
    }
  }

  #[test]
  fn uid_changes_never_reassign_previous_users_observed_traffic() {
    for metadata_gap in [false, true] {
      let mut reader = account();
      let before: NativeSnapshot =
        HashMap::from([(10, native(1, "/Applications/Browser.app/Contents/MacOS/Browser"))]).into();
      let time = Instant::now();
      reader.ingest(HashMap::from([(10, (100, 10))]), &before, &before, time);
      let measured =
        reader.ingest(HashMap::from([(10, (250, 30))]), &before, &before, time + INTERVAL);
      let old_instance = measured.groups[0].members[0].instance.clone();
      let old_group = measured.groups[0].id.clone();
      let mut after = before.clone();
      after.get_mut(&10).unwrap().uid += 1;
      let unknown =
        NativeSnapshot { verified: HashMap::new(), live: None, blockers: HashSet::new() };
      if metadata_gap {
        reader.ingest(HashMap::from([(10, (700, 70))]), &unknown, &unknown, time + INTERVAL * 2);
        assert_eq!(reader.previous[&10].totals.0, 150);
      }
      let hidden = reader.ingest(
        HashMap::from([(10, (900, 100))]),
        if metadata_gap { &unknown } else { &after },
        &after,
        time + INTERVAL * 3,
      );
      assert!(hidden.groups.is_empty());
      assert!(reader.quarantined.contains(&10));
      assert!(!reader.previous.contains_key(&10));
      let continuous =
        reader.ingest(HashMap::from([(10, (950, 110))]), &after, &after, time + INTERVAL * 4);
      assert!(continuous.groups.is_empty());
      reader.ingest(HashMap::new(), &after, &after, time + INTERVAL * 5);
      let fresh =
        reader.ingest(HashMap::from([(10, (1000, 120))]), &after, &after, time + INTERVAL * 6);
      assert_ne!(fresh.groups[0].members[0].instance, old_instance);
      assert_ne!(fresh.groups[0].id, old_group);
      assert!(fresh.groups[0].rx_bytes_per_sec.is_none());
      assert_eq!(fresh.groups[0].rx_session_bytes, 0);
      let next =
        reader.ingest(HashMap::from([(10, (1100, 140))]), &after, &after, time + INTERVAL * 7);
      assert_eq!(next.groups[0].rx_session_bytes, 100);
    }
  }

  #[test]
  fn uncertain_history_is_bounded_and_known_replacement_never_reuses_old_totals() {
    let mut reader = account();
    let time = Instant::now();
    let many: NativeSnapshot = (1..=MAX_PROCESSES as i32)
      .map(|pid| (pid, native(pid as u64, "/usr/bin/curl")))
      .collect::<HashMap<_, _>>()
      .into();
    reader.ingest(HashMap::from([(1, (100, 10))]), &many, &many, time);
    assert_eq!(reader.known.len(), MAX_PROCESSES);
    let unknown = NativeSnapshot { verified: HashMap::new(), live: None, blockers: HashSet::new() };
    let new: NativeSnapshot = (5000..5032)
      .map(|pid| (pid, native(pid as u64, "/usr/bin/curl")))
      .collect::<HashMap<_, _>>()
      .into();
    let new = NativeSnapshot { live: None, ..new };
    let hidden = reader.ingest(HashMap::from([(1, (250, 30))]), &new, &new, time + INTERVAL);
    assert!(hidden.groups.is_empty());
    assert_eq!(reader.known.len(), MAX_PROCESSES);
    assert!((5000..5032).all(|pid| reader.known.contains_key(&pid)));
    reader.ingest(HashMap::new(), &unknown, &unknown, time + INTERVAL * 2);
    assert_eq!(reader.known.len(), MAX_PROCESSES);
    assert!(reader.previous.len() <= MAX_PROCESSES);

    let mut reader = account();
    let old: NativeSnapshot = HashMap::from([(10, native(1, "/usr/bin/curl"))]).into();
    reader.ingest(HashMap::from([(10, (100, 10))]), &old, &old, time);
    let measured = reader.ingest(HashMap::from([(10, (250, 30))]), &old, &old, time + INTERVAL);
    let old_instance = measured.groups[0].members[0].instance.clone();
    let replacement: NativeSnapshot = HashMap::from([(10, native(2, "/usr/bin/curl"))]).into();
    let hidden =
      reader.ingest(HashMap::from([(10, (900, 100))]), &unknown, &replacement, time + INTERVAL * 2);
    assert!(hidden.groups.is_empty());
    assert!(!reader.previous.contains_key(&10));
    reader.ingest(HashMap::new(), &replacement, &replacement, time + INTERVAL * 3);
    let fresh = reader.ingest(
      HashMap::from([(10, (1000, 120))]),
      &replacement,
      &replacement,
      time + INTERVAL * 4,
    );
    assert_eq!(fresh.groups[0].rx_session_bytes, 0);
    assert_ne!(fresh.groups[0].members[0].instance, old_instance);
  }

  #[test]
  fn observer_input_stays_idle_on_its_private_terminal_and_shutdown_reaps_child() {
    let stop = AtomicBool::new(false);
    let mut pid = None;
    let started = Instant::now();
    let error = observe(
      Command::new("/usr/bin/python3").args(["-c", "import os,select,time; assert os.isatty(0) and os.isatty(1); assert os.ttyname(0)==os.ttyname(1); assert not select.select([0],[],[],0)[0]; print(',bytes_in,bytes_out,\\ncheck.%d,1,2,\\n,bytes_in,bytes_out,' % os.getpid(),flush=True); time.sleep(5)"]),
      &stop,
      Duration::from_millis(500),
      |rows, _, _, _| pid = rows.keys().next().copied(),
    ).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid = pid.expect("private stdin must be a terminal without immediately readable EOF");
    assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
  }

  #[test]
  fn stream_frames_fragmented_native_csv_and_preserves_header_sample_time() {
    let cancel = AtomicBool::new(false);
    let mut blocks = Vec::new();
    let result = observe(Command::new("/bin/sh").args(["-c", "printf ',bytes_in,bytes_out,\\n,bytes_tool.42,100,200,\\n'; printf ',bytes_out,bytes_in,\\nx.42,400,300,\\n'; exec /bin/sleep 5"]), &cancel, Duration::from_millis(120), |rows, _, _, sampled| blocks.push((rows, sampled)));
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].0[&42], (100, 200));
    assert!(blocks[0].1.elapsed() >= Duration::from_millis(100));
  }

  #[test]
  fn stopped_worker_reaps_child_and_bounds_partial_lines_and_slow_sources() {
    let stop = AtomicBool::new(true);
    let started = Instant::now();
    observe(Command::new("/bin/sleep").arg("5"), &stop, Duration::from_secs(1), |_, _, _, _| {
      panic!("no snapshot")
    })
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let stop = AtomicBool::new(false);
    let error = observe(
      Command::new("/bin/sh")
        .args(["-c", "printf ',bytes_in,bytes_out,\\n'; printf '%09000d' 1; exec /bin/sleep 5"]),
      &stop,
      Duration::from_secs(1),
      |_, _, _, _| panic!("partial data must never publish"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }
}
