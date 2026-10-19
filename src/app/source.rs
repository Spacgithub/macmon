//! Per-user, short-lived sampling owner. Consumers never create a fallback sampler.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use macmon::{Metrics, Sampler, SocInfo};
use serde::{Deserialize, Serialize};

use crate::procs::{ProcInfo, ProcSampler, ProcessSnapshot};
use crate::serve::{Cadence, SampleDetails};

const PROTOCOL: u32 = 1;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONNECTIONS: usize = 40;
const MAX_SUBSCRIBERS: usize = 16;
const IDLE_GRACE: Duration = Duration::from_millis(750);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProcessDemand {
  None,
  Summary,
  #[default]
  Details,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProcessState {
  Disabled,
  Warming,
  Ready,
  Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CadenceState {
  pub requested_ms: u32,
  pub effective_ms: u32,
  pub pending: bool,
  // Missing on older owners. Their zero-generation messages keep the old ordering behavior.
  #[serde(default)]
  pub generation: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ProcessFrame {
  pub sequence: u64,
  pub rows: Vec<ProcInfo>,
  pub snapshot: ProcessSnapshot,
  // Public groups deliberately omit Terminal-only columns. Keep those in private IPC only.
  extras: Vec<(Option<f32>, usize, Option<u64>)>,
}

impl ProcessFrame {
  #[cfg(test)]
  pub(crate) fn fixture(snapshot: ProcessSnapshot) -> Self {
    let extras = snapshot
      .groups
      .iter()
      .map(|group| (group.power_w, group.power_known, group.cpu_time_ns))
      .collect();
    Self { sequence: 1, rows: Vec::new(), snapshot, extras }
  }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Frame {
  pub session: String,
  pub collector_pid: u32,
  pub revision: u64,
  pub sequence: u64,
  pub soc: SocInfo,
  pub metrics: Metrics,
  pub details: SampleDetails,
  pub sampled_at: String,
  #[serde(with = "instant")]
  pub sampled: Instant,
  pub interval_ms: u32,
  pub cadence: CadenceState,
  pub process_interval_ms: u32,
  pub process_state: ProcessState,
  #[serde(default)]
  pub process_demand: ProcessDemand,
  #[serde(with = "optional_arc")]
  pub processes: Option<Arc<ProcessFrame>>,
}

/// Same-host IPC uses Apple's awake monotonic clock, matching Rust's Instant on macOS.
/// https://doc.rust-lang.org/src/std/sys/time/unix.rs.html
pub(crate) mod instant {
  use super::*;
  fn uptime() -> io::Result<u64> {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut time) } != 0 {
      return Err(io::Error::last_os_error());
    }
    u64::try_from(time.tv_sec)
      .ok()
      .and_then(|seconds| seconds.checked_mul(1_000_000_000))
      .and_then(|seconds| seconds.checked_add(time.tv_nsec as u64))
      .ok_or_else(|| invalid("invalid monotonic clock"))
  }
  pub fn serialize<S: serde::Serializer>(
    value: &Instant,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    let clock = uptime().map_err(serde::ser::Error::custom)?;
    let age = value.elapsed().as_nanos();
    let timestamp = u64::try_from(age)
      .ok()
      .and_then(|age| clock.checked_sub(age))
      .ok_or_else(|| serde::ser::Error::custom("sample predates uptime clock"))?;
    serializer.serialize_u64(timestamp)
  }
  pub fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
  ) -> Result<Instant, D::Error> {
    let timestamp = u64::deserialize(deserializer)?;
    let now = Instant::now();
    let clock = uptime().map_err(serde::de::Error::custom)?;
    let age = clock
      .checked_sub(timestamp)
      .ok_or_else(|| serde::de::Error::custom("future sample timestamp"))?;
    now
      .checked_sub(Duration::from_nanos(age))
      .ok_or_else(|| serde::de::Error::custom("invalid sample timestamp"))
  }
}

mod optional_arc {
  use super::*;
  pub fn serialize<S: serde::Serializer>(
    value: &Option<Arc<ProcessFrame>>,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    value.as_deref().serialize(serializer)
  }
  pub fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
  ) -> Result<Option<Arc<ProcessFrame>>, D::Error> {
    Option::<ProcessFrame>::deserialize(deserializer).map(|value| value.map(Arc::new))
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
  Connecting,
  Live,
  Disconnected,
  Incompatible,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Request {
  Subscribe { protocol: u32, id: String, initial_ms: Option<u32>, demand: ProcessDemand },
  Adjust { protocol: u32, session: String, adjustment: String },
  SetInterval { protocol: u32, session: String, interval_ms: u32 },
  Demand { protocol: u32, session: String, id: String, demand: ProcessDemand },
}

#[derive(Serialize, Deserialize)]
struct Welcome {
  protocol: u32,
  session: String,
  cadence: CadenceState,
}

#[derive(Clone)]
struct Runtime {
  directory: PathBuf,
}

impl Runtime {
  fn default_directory(uid: u32) -> PathBuf {
    PathBuf::from(format!("/private/tmp/macmon-{uid}"))
  }
  fn open() -> io::Result<Self> {
    let uid = unsafe { libc::geteuid() };
    let directory = std::env::var_os("MACMON_RUNTIME_DIR")
      .map(PathBuf::from)
      .unwrap_or_else(|| Self::default_directory(uid));
    Self::at(directory)
  }
  fn at(directory: PathBuf) -> io::Result<Self> {
    let uid = unsafe { libc::geteuid() };
    if !directory.is_absolute() {
      return Err(invalid("runtime directory must be absolute"));
    }
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
      Ok(()) => {}
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
      Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir()
      || metadata.file_type().is_symlink()
      || metadata.uid() != uid
      || metadata.mode() & 0o077 != 0
    {
      return Err(invalid("runtime directory must be a private, owned directory, not a symlink"));
    }
    Ok(Self { directory })
  }
  fn socket(&self) -> PathBuf {
    self.directory.join("collector.sock")
  }
  fn connect(&self) -> io::Result<UnixStream> {
    let metadata = fs::symlink_metadata(self.socket())?;
    if !metadata.file_type().is_socket()
      || metadata.uid() != unsafe { libc::geteuid() }
      || metadata.mode() & 0o077 != 0
    {
      return Err(invalid("unowned collector socket"));
    }
    let stream = UnixStream::connect(self.socket())?;
    same_user(&stream)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
  }
  fn owner_lock(&self) -> io::Result<File> {
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(self.directory.join("collector.lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
      || metadata.uid() != unsafe { libc::geteuid() }
      || metadata.mode() & 0o077 != 0
    {
      return Err(invalid("unowned collector lock"));
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(file)
  }
}

fn invalid(message: &str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message)
}
fn same_user(stream: &UnixStream) -> io::Result<()> {
  let (mut uid, mut gid) = (0, 0);
  if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
    return Err(io::Error::last_os_error());
  }
  if uid != unsafe { libc::geteuid() } {
    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "collector peer has another UID"));
  }
  Ok(())
}
fn unique() -> String {
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  format!(
    "{}-{}-{}",
    std::process::id(),
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(),
    COUNTER.fetch_add(1, Ordering::Relaxed)
  )
}
fn write_packet(stream: &mut UnixStream, kind: u8, bytes: &[u8]) -> io::Result<()> {
  if bytes.len() > MAX_FRAME {
    return Err(invalid("collector frame is too large"));
  }
  stream.write_all(&[kind])?;
  stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
  stream.write_all(bytes)
}
fn write_json(stream: &mut UnixStream, kind: u8, value: &impl Serialize) -> io::Result<()> {
  write_packet(stream, kind, &serde_json::to_vec(value)?)
}
fn read_packet(stream: &mut UnixStream) -> io::Result<(u8, Vec<u8>)> {
  read_limited_packet(stream, MAX_FRAME)
}
fn read_limited_packet(stream: &mut UnixStream, limit: usize) -> io::Result<(u8, Vec<u8>)> {
  let mut header = [0; 5];
  stream.read_exact(&mut header)?;
  let length = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
  if length > limit {
    return Err(invalid("collector frame is too large"));
  }
  let mut bytes = vec![0; length];
  stream.read_exact(&mut bytes)?;
  Ok((header[0], bytes))
}

struct ClientData {
  frame: Option<Arc<Frame>>,
  state: ConnectionState,
  cadence: CadenceState,
  session: String,
  error: String,
  socket: Option<UnixStream>,
}

impl ClientData {
  fn accept_cadence(&mut self, session: &str, cadence: CadenceState) -> bool {
    if self.session != session || cadence.generation < self.cadence.generation {
      return false;
    }
    if self.cadence != cadence {
      self.cadence = cadence;
      if let Some(frame) = self.frame.as_mut().filter(|frame| frame.session == session) {
        Arc::make_mut(frame).cadence = cadence;
      }
    }
    true
  }

  fn welcome(&mut self, session: &str, cadence: CadenceState) {
    if self.session != session {
      self.session = session.into();
      self.cadence = cadence;
    } else {
      self.accept_cadence(session, cadence);
    }
  }
}
struct ClientInner {
  data: Mutex<ClientData>,
  changed: Condvar,
  stopped: AtomicBool,
  demand: Mutex<ProcessDemand>,
  runtime: Runtime,
  id: String,
  default_ms: u32,
}

pub(crate) struct Client {
  inner: Arc<ClientInner>,
}

impl Client {
  pub fn connect(initial_ms: Option<u32>, demand: ProcessDemand) -> io::Result<Self> {
    Self::connect_role(initial_ms, demand, true, 1000)
  }
  pub fn connect_server(initial_ms: Option<u32>) -> io::Result<Self> {
    Self::connect(initial_ms, ProcessDemand::None)
  }
  pub fn connect_tui(
    default_ms: u32,
    initial_ms: Option<u32>,
    demand: ProcessDemand,
  ) -> io::Result<Self> {
    Self::connect_role(initial_ms, demand, false, default_ms)
  }
  fn connect_role(
    initial_ms: Option<u32>,
    demand: ProcessDemand,
    server: bool,
    default_ms: u32,
  ) -> io::Result<Self> {
    let runtime = Runtime::open()?;
    if !server && std::env::var_os("MACMON_RUNTIME_DIR").is_none() && runtime.connect().is_err() {
      crate::web::require_shareable_backend()?;
    }
    Self::at(runtime, initial_ms, demand, default_ms)
  }
  fn at(
    runtime: Runtime,
    initial_ms: Option<u32>,
    demand: ProcessDemand,
    default_ms: u32,
  ) -> io::Result<Self> {
    let inner = Arc::new(ClientInner {
      data: Mutex::new(ClientData {
        frame: None,
        state: ConnectionState::Connecting,
        cadence: CadenceState::default(),
        session: String::new(),
        error: String::new(),
        socket: None,
      }),
      changed: Condvar::new(),
      stopped: AtomicBool::new(false),
      demand: Mutex::new(demand),
      runtime,
      id: unique(),
      default_ms,
    });
    let worker = Arc::clone(&inner);
    thread::spawn(move || client_worker(worker, initial_ms));
    Ok(Self { inner })
  }
  pub fn latest(&self) -> Option<Arc<Frame>> {
    self.inner.data.lock().unwrap().frame.clone()
  }
  pub fn status(&self) -> ConnectionState {
    self.inner.data.lock().unwrap().state
  }
  pub fn cadence(&self) -> CadenceState {
    self.inner.data.lock().unwrap().cadence
  }
  pub fn wait_after(
    &self,
    previous: Option<(&str, u64)>,
    timeout: Duration,
  ) -> io::Result<Option<Arc<Frame>>> {
    let changed = |data: &ClientData| {
      data
        .frame
        .as_ref()
        .is_some_and(|frame| previous != Some((frame.session.as_str(), frame.revision)))
    };
    let data = self.inner.data.lock().unwrap();
    let (data, _) = self
      .inner
      .changed
      .wait_timeout_while(data, timeout, |data| {
        !changed(data) && data.state != ConnectionState::Incompatible
      })
      .unwrap();
    if data.state == ConnectionState::Incompatible {
      return Err(invalid(&data.error));
    }
    Ok(data.frame.as_ref().filter(|_| changed(&data)).cloned())
  }
  pub fn adjust(&self, adjustment: &str) -> io::Result<CadenceState> {
    if !matches!(adjustment, "increase" | "decrease") {
      return Err(invalid("invalid interval adjustment"));
    }
    let session = self.session()?;
    self.control(
      &session,
      Request::Adjust {
        protocol: PROTOCOL,
        session: session.clone(),
        adjustment: adjustment.into(),
      },
    )
  }
  pub fn set_demand(&self, demand: ProcessDemand) -> io::Result<()> {
    *self.inner.demand.lock().unwrap() = demand;
    Ok(())
  }
  fn session(&self) -> io::Result<String> {
    let data = self.inner.data.lock().unwrap();
    if data.state != ConnectionState::Live {
      return Err(io::Error::new(io::ErrorKind::NotConnected, "sampling source is disconnected"));
    }
    Ok(data.session.clone())
  }
  fn control(&self, session: &str, request: Request) -> io::Result<CadenceState> {
    let mut stream = self.inner.runtime.connect()?;
    write_json(&mut stream, b'Q', &request)?;
    let (kind, bytes) = read_packet(&mut stream)?;
    if kind != b'C' {
      return Err(invalid("collector rejected control"));
    }
    let cadence = serde_json::from_slice(&bytes)?;
    let mut data = self.inner.data.lock().unwrap();
    if data.session != session {
      return Err(io::Error::new(
        io::ErrorKind::NotConnected,
        "sampling source changed during control",
      ));
    }
    data.accept_cadence(session, cadence);
    let cadence = data.cadence;
    drop(data);
    self.inner.changed.notify_all();
    Ok(cadence)
  }
}
impl Drop for Client {
  fn drop(&mut self) {
    self.inner.stopped.store(true, Ordering::Release);
    if let Some(socket) = &self.inner.data.lock().unwrap().socket {
      let _ = socket.shutdown(Shutdown::Both);
    }
    self.inner.changed.notify_all();
  }
}

fn collector_binary() -> io::Result<PathBuf> {
  #[cfg(test)]
  if let Some(binary) = std::env::var_os("MACMON_TEST_COLLECTOR_BIN") {
    return Ok(binary.into());
  }
  std::env::current_exe()
}
fn spawn_owner(runtime: &Runtime, initial_ms: u32) -> io::Result<()> {
  use std::os::unix::process::CommandExt;
  let mut command = Command::new(collector_binary()?);
  command
    .args(["__collector", "--initial-ms", &initial_ms.to_string()])
    .env("MACMON_RUNTIME_DIR", &runtime.directory)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  // It must survive the first Terminal closing while another consumer still holds a lease.
  unsafe {
    command.pre_exec(|| {
      if libc::setsid() < 0 {
        return Err(io::Error::last_os_error());
      }
      Ok(())
    });
  }
  let mut child = command.spawn()?;
  thread::spawn(move || {
    let _ = child.wait();
  });
  Ok(())
}
fn replacement_interval(shared: u32, explicit: Option<u32>, fallback: u32) -> u32 {
  if shared >= 100 { shared } else { explicit.filter(|value| *value >= 100).unwrap_or(fallback) }
}

fn client_worker(inner: Arc<ClientInner>, mut initial_ms: Option<u32>) {
  while !inner.stopped.load(Ordering::Acquire) {
    let result = (|| -> io::Result<()> {
      let mut stream = match inner.runtime.connect() {
        Ok(stream) => stream,
        Err(error)
          if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
        {
          let shared = inner.data.lock().unwrap().cadence.requested_ms;
          spawn_owner(&inner.runtime, replacement_interval(shared, initial_ms, inner.default_ms))?;
          let deadline = Instant::now() + Duration::from_secs(5);
          loop {
            match inner.runtime.connect() {
              Ok(stream) => break stream,
              Err(error) if Instant::now() < deadline && !inner.stopped.load(Ordering::Acquire) => {
                let _ = error;
                thread::sleep(Duration::from_millis(40));
              }
              Err(error) => return Err(error),
            }
          }
        }
        Err(error) => return Err(error),
      };
      let subscribed_demand = *inner.demand.lock().unwrap();
      write_json(
        &mut stream,
        b'Q',
        &Request::Subscribe {
          protocol: PROTOCOL,
          id: inner.id.clone(),
          initial_ms,
          demand: subscribed_demand,
        },
      )?;
      let (kind, bytes) = read_packet(&mut stream)?;
      if kind != b'H' {
        return Err(invalid("incompatible sampling owner"));
      }
      let welcome: Welcome = serde_json::from_slice(&bytes)?;
      if welcome.protocol != PROTOCOL {
        return Err(invalid("incompatible sampling protocol"));
      }
      let session = welcome.session;
      initial_ms = None; // Reconnects never replay an old CLI preference over another consumer.
      {
        let mut data = inner.data.lock().unwrap();
        data.welcome(&session, welcome.cadence);
        data.socket = Some(stream.try_clone()?);
      }
      let mut sent_demand = subscribed_demand;
      while !inner.stopped.load(Ordering::Acquire) {
        let demand = *inner.demand.lock().unwrap();
        if demand != sent_demand {
          let mut control = inner.runtime.connect()?;
          let session = inner.data.lock().unwrap().session.clone();
          write_json(
            &mut control,
            b'Q',
            &Request::Demand { protocol: PROTOCOL, session, id: inner.id.clone(), demand },
          )?;
          let (kind, _) = read_packet(&mut control)?;
          if kind != b'C' {
            return Err(invalid("collector rejected demand"));
          }
          sent_demand = demand;
        }
        let (kind, bytes) = read_packet(&mut stream)?;
        match kind {
          b'F' => {
            let mut frame: Frame = serde_json::from_slice(&bytes)?;
            if let Some(processes) = frame.processes.as_mut().and_then(Arc::get_mut) {
              if processes.extras.len() != processes.snapshot.groups.len() {
                return Err(invalid("incomplete process snapshot"));
              }
              for (group, &(power, known, time)) in
                processes.snapshot.groups.iter_mut().zip(&processes.extras)
              {
                group.power_w = power;
                group.power_known = known;
                group.cpu_time_ns = time;
              }
            }
            let mut data = inner.data.lock().unwrap();
            if frame.session != session {
              return Err(invalid("collector frame belongs to another session"));
            }
            data.accept_cadence(&session, frame.cadence);
            frame.cadence = data.cadence;
            data.frame = Some(Arc::new(frame));
            data.state = ConnectionState::Live;
          }
          b'C' => {
            let cadence = serde_json::from_slice(&bytes)?;
            inner.data.lock().unwrap().accept_cadence(&session, cadence);
          }
          _ => return Err(invalid("invalid collector response")),
        }
        inner.changed.notify_all();
      }
      Ok(())
    })();
    let mut data = inner.data.lock().unwrap();
    data.socket = None;
    if let Err(error) = result {
      data.state = if error.kind() == io::ErrorKind::InvalidData {
        ConnectionState::Incompatible
      } else {
        ConnectionState::Disconnected
      };
      data.error = error.to_string();
    }
    inner.changed.notify_all();
    if data.state == ConnectionState::Incompatible {
      break;
    }
    drop(data);
    thread::sleep(Duration::from_millis(150));
  }
}

struct Encoded {
  bytes: Vec<u8>,
  revision: u64,
}
struct OwnerData {
  cadence: Cadence,
  leases: HashMap<String, (ProcessDemand, String)>,
  last_empty: Instant,
  sequence: u64,
  revision: u64,
  frame: Option<Frame>,
  latest: Option<Arc<Encoded>>,
  processes: Option<Arc<ProcessFrame>>,
  process_demand: ProcessDemand,
  app_network: Option<crate::app_network::Snapshot>,
  process_epoch: u64,
  process_state: ProcessState,
  error: Option<String>,
}

impl OwnerData {
  fn refresh_process_demand(&mut self) {
    let demand = self.leases.values().map(|lease| lease.0).max().unwrap_or(ProcessDemand::None);
    if (demand == ProcessDemand::None) != (self.process_demand == ProcessDemand::None) {
      self.process_epoch = self.process_epoch.wrapping_add(1);
      self.process_state =
        if demand == ProcessDemand::None { ProcessState::Disabled } else { ProcessState::Warming };
    }
    self.process_demand = demand;
  }
}
struct Owner {
  _lock: File,
  publishing: Mutex<()>,
  connections: AtomicUsize,
  data: Mutex<OwnerData>,
  changed: Condvar,
  session: String,
  stopped: AtomicBool,
}

impl Owner {
  fn new(lock: File, initial_ms: u32) -> Self {
    Self {
      _lock: lock,
      publishing: Mutex::new(()),
      connections: AtomicUsize::new(0),
      data: Mutex::new(OwnerData {
        cadence: Cadence::new(initial_ms.max(100)),
        leases: HashMap::new(),
        last_empty: Instant::now(),
        sequence: 0,
        revision: 0,
        frame: None,
        latest: None,
        processes: None,
        process_demand: ProcessDemand::None,
        app_network: None,
        process_epoch: 0,
        process_state: ProcessState::Disabled,
        error: None,
      }),
      changed: Condvar::new(),
      session: unique(),
      stopped: AtomicBool::new(false),
    }
  }
  fn publish(&self, system: Option<Frame>, processes: Option<Arc<ProcessFrame>>) -> io::Result<()> {
    self.publish_checked(system, processes, None)
  }
  fn publish_checked(
    &self,
    system: Option<Frame>,
    processes: Option<Arc<ProcessFrame>>,
    expected: Option<(u64, bool)>,
  ) -> io::Result<()> {
    // Only publishers serialize; control and lease requests never wait on JSON/FFI work.
    let _publishing = self.publishing.lock().unwrap();
    let mut frame = {
      let mut data = self.data.lock().unwrap();
      if let Some((epoch, first)) = expected {
        if data.process_epoch != epoch
          || data.process_demand == ProcessDemand::None
          || self.stopped.load(Ordering::Acquire)
        {
          return Ok(());
        }
        data.process_state = if first { ProcessState::Warming } else { ProcessState::Ready };
      }
      if let Some(processes) = processes {
        data.processes = Some(processes);
      }
      let system_sample = system.is_some();
      let Some(mut frame) = system.or_else(|| data.frame.clone()) else { return Ok(()) };
      frame.sequence = data.sequence + u64::from(system_sample);
      frame.revision = data.revision + 1;
      frame.processes = data.processes.clone();
      frame.details.app_network.clone_from(&data.app_network);
      frame.process_state = data.process_state;
      frame.process_demand = data.process_demand;
      frame.cadence = data.cadence.state();
      frame.process_interval_ms =
        crate::process_demand::interval_ms(data.process_demand, frame.cadence.effective_ms)
          .unwrap_or(0);
      frame
    };
    let packet = Arc::new(Encoded { revision: frame.revision, bytes: serde_json::to_vec(&frame)? });
    if packet.bytes.len() > MAX_FRAME {
      return Err(invalid("sampling frame exceeds IPC limit"));
    }
    let mut data = self.data.lock().unwrap();
    data.sequence = frame.sequence;
    data.revision = frame.revision;
    data.latest = Some(packet);
    // Keep latest requested cadence outside immutable system sample identity.
    frame.cadence = data.cadence.state();
    data.frame = Some(frame);
    drop(data);
    self.changed.notify_all();
    Ok(())
  }
}

fn bind_private_socket(runtime: &Runtime) -> io::Result<UnixListener> {
  // Only the hidden owner (or an isolated test child) calls this, before starting worker threads.
  unsafe {
    libc::umask(0o077);
  }
  UnixListener::bind(runtime.socket())
}

/// Hidden owner command; the lock is acquired before any hardware sampler is constructed.
pub(crate) fn run(initial_ms: u32) -> io::Result<()> {
  let runtime = Runtime::open()?;
  let _lock = match runtime.owner_lock() {
    Ok(lock) => lock,
    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
    Err(error) => return Err(error),
  };
  if runtime.socket().exists() {
    if runtime.connect().is_ok() {
      return Err(invalid("an unknown owner already uses the runtime socket"));
    }
    let metadata = fs::symlink_metadata(runtime.socket())?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
      return Err(invalid("unowned runtime socket"));
    }
    fs::remove_file(runtime.socket())?;
  }
  let listener = bind_private_socket(&runtime)?;
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600))?;
  listener.set_nonblocking(true)?;
  let owner = Arc::new(Owner::new(_lock, initial_ms));
  owner.data.lock().unwrap().app_network = Some(Default::default());
  let network_owner = Arc::clone(&owner);
  let _network = match crate::app_network::Worker::start(move |snapshot| {
    network_owner.data.lock().unwrap().app_network = Some(snapshot);
    let _ = network_owner.publish(None, None);
  }) {
    Ok(worker) => Some(worker),
    Err(_) => {
      owner.data.lock().unwrap().app_network.as_mut().unwrap().state = "unavailable".into();
      None
    }
  };
  let system = Arc::clone(&owner);
  thread::spawn(move || {
    if let Err(error) = sample_system(&system) {
      system.data.lock().unwrap().error = Some(error.to_string());
      system.changed.notify_all();
    }
  });
  let processes = Arc::clone(&owner);
  thread::spawn(move || sample_processes(&processes));
  loop {
    match listener.accept() {
      Ok((stream, _)) => {
        if owner.connections.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
          owner.connections.fetch_sub(1, Ordering::AcqRel);
          continue;
        }
        let owner = Arc::clone(&owner);
        thread::spawn(move || {
          let _ = owner_connection(stream, Arc::clone(&owner));
          owner.connections.fetch_sub(1, Ordering::AcqRel);
        });
      }
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
      Err(error) => return Err(error),
    }
    let idle = {
      let data = owner.data.lock().unwrap();
      data.leases.is_empty() && data.last_empty.elapsed() >= IDLE_GRACE
    };
    if idle || owner.data.lock().unwrap().error.is_some() {
      break;
    }
    thread::sleep(Duration::from_millis(20));
  }
  owner.stopped.store(true, Ordering::Release);
  owner.changed.notify_all();
  fs::remove_file(runtime.socket())?;
  if let Some(error) = owner.data.lock().unwrap().error.take() {
    return Err(io::Error::other(error));
  }
  Ok(())
}

fn owner_connection(mut stream: UnixStream, owner: Arc<Owner>) -> io::Result<()> {
  // Darwin accepts inherit the listener's nonblocking flag; a live lease reader must block.
  stream.set_nonblocking(false)?;
  same_user(&stream)?;
  stream.set_read_timeout(Some(IO_TIMEOUT))?;
  stream.set_write_timeout(Some(IO_TIMEOUT))?;
  let (kind, bytes) = read_limited_packet(&mut stream, 1024)?;
  if kind != b'Q' {
    return Err(invalid("invalid collector request"));
  }
  let request: Request = serde_json::from_slice(&bytes)?;
  match request {
    Request::Subscribe { protocol, id, initial_ms, demand }
      if protocol == PROTOCOL && id.len() < 128 =>
    {
      let reader = stream.try_clone()?;
      reader.set_read_timeout(None)?;
      let generation = unique();
      let cadence = {
        let mut data = owner.data.lock().unwrap();
        if data.leases.len() >= MAX_SUBSCRIBERS && !data.leases.contains_key(&id) {
          return Err(invalid("collector subscriber limit"));
        }
        if let Some(initial) = initial_ms {
          data.cadence.set(initial.max(100));
        }
        data.leases.insert(id.clone(), (demand, generation.clone()));
        data.refresh_process_demand();
        data.cadence.state()
      };
      owner.changed.notify_all();
      owner.publish(None, None)?;
      let closed = Arc::new(AtomicBool::new(false));
      let disconnect = Arc::clone(&closed);
      let leases = Arc::clone(&owner);
      thread::spawn(move || {
        let mut reader = reader;
        let mut byte = [0];
        let _ = reader.read(&mut byte);
        disconnect.store(true, Ordering::Release);
        let mut data = leases.data.lock().unwrap();
        if data.leases.get(&id).is_some_and(|lease| lease.1 == generation) {
          data.leases.remove(&id);
        }
        data.refresh_process_demand();
        if data.leases.is_empty() {
          data.last_empty = Instant::now();
        }
        leases.changed.notify_all();
        drop(data);
        let _ = leases.publish(None, None);
      });
      let result = (|| {
        write_json(
          &mut stream,
          b'H',
          &Welcome { protocol: PROTOCOL, session: owner.session.clone(), cadence },
        )?;
        let mut revision = 0;
        while !closed.load(Ordering::Acquire) && !owner.stopped.load(Ordering::Acquire) {
          let data = owner.data.lock().unwrap();
          let (data, _) = owner
            .changed
            .wait_timeout_while(data, Duration::from_millis(500), |data| {
              data.revision == revision && data.error.is_none() && !closed.load(Ordering::Acquire)
            })
            .unwrap();
          if let Some(error) = &data.error {
            return Err(invalid(error));
          }
          let packet = data.latest.as_ref().filter(|packet| packet.revision != revision).cloned();
          let cadence = data.cadence.state();
          drop(data);
          if let Some(packet) = packet {
            write_packet(&mut stream, b'F', &packet.bytes)?;
            revision = packet.revision;
          }
          write_json(&mut stream, b'C', &cadence)?;
        }
        Ok(())
      })();
      let _ = stream.shutdown(Shutdown::Both);
      result
    }
    request => {
      let mut data = owner.data.lock().unwrap();
      let accepted = match request {
        Request::Adjust { protocol, session, adjustment }
          if protocol == PROTOCOL && session == owner.session =>
        {
          data.cadence.adjust(&adjustment)
        }
        Request::SetInterval { protocol, session, interval_ms }
          if protocol == PROTOCOL && session == owner.session && interval_ms >= 100 =>
        {
          data.cadence.set(interval_ms);
          true
        }
        Request::Demand { protocol, session, id, demand }
          if protocol == PROTOCOL && session == owner.session =>
        {
          data.leases.get_mut(&id).is_some_and(|value| {
            value.0 = demand;
            true
          })
        }
        _ => false,
      };
      let cadence = data.cadence.state();
      data.refresh_process_demand();
      drop(data);
      owner.changed.notify_all();
      owner.publish(None, None)?;
      if accepted {
        write_json(&mut stream, b'C', &cadence)
      } else {
        Err(invalid("collector rejected request"))
      }
    }
  }
}

fn sample_system(owner: &Owner) -> io::Result<()> {
  {
    let data = owner.data.lock().unwrap();
    let _guard = owner
      .changed
      .wait_while(data, |data| data.leases.is_empty() && !owner.stopped.load(Ordering::Acquire))
      .unwrap();
  }
  if owner.stopped.load(Ordering::Acquire) {
    return Ok(());
  }
  let mut sampler = Sampler::new().map_err(|error| io::Error::other(error.to_string()))?;
  let soc = sampler.get_soc_info().clone();
  let mut window = 100;
  let mut failures = 0;
  while !owner.stopped.load(Ordering::Acquire) {
    let metrics = match sampler.get_metrics(window) {
      Ok(metrics) => {
        failures = 0;
        metrics
      }
      Err(error) => {
        failures += 1;
        if failures >= 3 {
          return Err(io::Error::other(error.to_string()));
        }
        thread::sleep(Duration::from_millis(250 * failures));
        continue;
      }
    };
    let (sampled, sampled_at) = crate::serve::sample_time();
    let details = SampleDetails {
      temperatures: sampler.temperature_details().to_vec(),
      memory_activity: *sampler.memory_activity(),
      thermal_state: sampler.thermal_state(),
      disks: sampler.disks().map(<[_]>::to_vec),
      app_network: None,
    };
    let (processes, cadence) = {
      let data = owner.data.lock().unwrap();
      (data.processes.clone(), data.cadence.state())
    };
    let process_state =
      if processes.is_some() { ProcessState::Ready } else { ProcessState::Warming };
    owner.publish(
      Some(Frame {
        session: owner.session.clone(),
        collector_pid: std::process::id(),
        revision: 0,
        sequence: 0,
        soc: soc.clone(),
        metrics,
        details,
        sampled_at,
        sampled,
        interval_ms: window,
        cadence,
        process_interval_ms: cadence.effective_ms,
        process_state,
        process_demand: ProcessDemand::None,
        processes,
      }),
      None,
    )?;
    let previous = owner.data.lock().unwrap().cadence.state().effective_ms;
    window = owner.data.lock().unwrap().cadence.begin();
    if previous != window {
      owner.publish(None, None)?;
    }
  }
  Ok(())
}
fn sample_processes(owner: &Owner) {
  sample_processes_with(owner, ProcSampler::new);
}

trait ProcessReader {
  fn sample(&mut self) -> Vec<ProcInfo>;
  fn reset_rates(&mut self);
}
impl ProcessReader for ProcSampler {
  fn sample(&mut self) -> Vec<ProcInfo> {
    ProcSampler::sample(self)
  }
  fn reset_rates(&mut self) {
    ProcSampler::reset_rates(self);
  }
}

fn sample_processes_with<S: ProcessReader>(owner: &Owner, mut factory: impl FnMut() -> S) {
  let mut sampler = None;
  let mut epoch = 0;
  let mut last = None::<Instant>;
  let mut sequence = 0;
  let mut warming = true;
  while !owner.stopped.load(Ordering::Acquire) {
    let mut data = owner.data.lock().unwrap();
    let interval =
      crate::process_demand::interval_ms(data.process_demand, data.cadence.state().effective_ms);
    if epoch != data.process_epoch {
      if let Some(sampler) = &mut sampler {
        ProcessReader::reset_rates(sampler);
      }
      warming = true;
      last = None;
      epoch = data.process_epoch;
    }
    let Some(interval) = interval else {
      last = None;
      drop(owner.changed.wait_timeout(data, Duration::from_secs(1)).unwrap());
      continue;
    };
    if let Some(completed) = last {
      let wait = Duration::from_millis(interval.into()).saturating_sub(completed.elapsed());
      if !wait.is_zero() {
        drop(owner.changed.wait_timeout(data, wait).unwrap());
        continue;
      }
    }
    drop(data);
    let first = warming;
    let rows = sampler.get_or_insert_with(&mut factory).sample();
    last = Some(Instant::now());
    data = owner.data.lock().unwrap();
    if owner.stopped.load(Ordering::Acquire)
      || epoch != data.process_epoch
      || data.process_demand == ProcessDemand::None
    {
      continue;
    }
    if rows.is_empty() {
      data.process_state = ProcessState::Failed;
      sampler.as_mut().unwrap().reset_rates();
      warming = true;
      drop(data);
      let _ = owner.publish(None, None);
      continue;
    }
    warming = false;
    drop(data);
    let snapshot = ProcSampler::snapshot(&rows, Instant::now());
    let extras = snapshot
      .groups
      .iter()
      .map(|group| (group.power_w, group.power_known, group.cpu_time_ns))
      .collect();
    sequence += 1;
    if let Err(error) = owner.publish_checked(
      None,
      Some(Arc::new(ProcessFrame { sequence, rows, snapshot, extras })),
      Some((epoch, first)),
    ) {
      owner.data.lock().unwrap().error = Some(error.to_string());
      owner.changed.notify_all();
      break;
    }
  }
}

#[cfg(test)]
impl Frame {
  pub(crate) fn fixture(sequence: u64, revision: u64) -> Self {
    Self {
      session: "fixture".into(),
      collector_pid: std::process::id(),
      sequence,
      revision,
      soc: SocInfo::default(),
      metrics: Metrics::default(),
      details: SampleDetails::default(),
      sampled_at: "2026-10-18T00:00:00Z".into(),
      sampled: Instant::now(),
      interval_ms: 1000,
      cadence: CadenceState {
        requested_ms: 1000,
        effective_ms: 1000,
        pending: false,
        generation: 0,
      },
      process_interval_ms: 1000,
      process_state: ProcessState::Warming,
      process_demand: ProcessDemand::Details,
      processes: None,
    }
  }
}

#[cfg(test)]
mod tests {
  use std::os::unix::fs::{PermissionsExt, symlink};

  use super::*;

  struct FakeReader<F>(F);
  impl<F: FnMut() -> Vec<ProcInfo>> ProcessReader for FakeReader<F> {
    fn sample(&mut self) -> Vec<ProcInfo> {
      (self.0)()
    }
    fn reset_rates(&mut self) {}
  }

  fn fake_process() -> ProcInfo {
    ProcInfo {
      pid: 42,
      name: "fixture".into(),
      path: "/fixture".into(),
      user: "fixture".into(),
      cpu_pct: 0.0,
      gpu_pct: 0.0,
      cpu_time_ns: 0,
      mem_bytes: 1024,
      power_w: None,
      started: Some((1, 0)),
      meta: Default::default(),
    }
  }

  fn demand(owner: &Owner, demand: ProcessDemand) {
    let mut data = owner.data.lock().unwrap();
    data.leases.insert("fixture".into(), (demand, "generation".into()));
    data.refresh_process_demand();
    drop(data);
    owner.changed.notify_all();
  }

  #[test]
  fn process_worker_stops_without_demand_and_restarts_with_a_fresh_full_baseline() {
    let dir = Directory::new();
    let owner = dir.owner();
    owner.publish(Some(Frame::fixture(0, 0)), None).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let factories = Arc::new(AtomicUsize::new(0));
    let worker = {
      let (owner, reads, factories) = (owner.clone(), reads.clone(), factories.clone());
      thread::spawn(move || {
        sample_processes_with(&owner, || {
          factories.fetch_add(1, Ordering::Relaxed);
          let reads = reads.clone();
          FakeReader(move || {
            reads.fetch_add(1, Ordering::Relaxed);
            vec![fake_process()]
          })
        })
      })
    };
    thread::sleep(Duration::from_millis(30));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    demand(&owner, ProcessDemand::Summary);
    until(|| owner.data.lock().unwrap().processes.is_some());
    let cached = owner.data.lock().unwrap().processes.clone().unwrap();
    assert_eq!(owner.data.lock().unwrap().process_state, ProcessState::Warming);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    demand(&owner, ProcessDemand::None);
    owner.publish(None, None).unwrap();
    let data = owner.data.lock().unwrap();
    assert!(Arc::ptr_eq(data.processes.as_ref().unwrap(), &cached));
    assert_eq!(data.frame.as_ref().unwrap().process_state, ProcessState::Disabled);
    assert_eq!(data.frame.as_ref().unwrap().process_interval_ms, 0);
    drop(data);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    demand(&owner, ProcessDemand::Details);
    until(|| factories.load(Ordering::Relaxed) == 1 && reads.load(Ordering::Relaxed) == 2);
    until(|| owner.data.lock().unwrap().processes.as_ref().unwrap().sequence == 2);
    assert!(
      serde_json::to_value(
        &owner.data.lock().unwrap().processes.as_ref().unwrap().snapshot.rows[0]
      )
      .unwrap()["cpu_pct"]
        .is_null()
    );
    owner.stopped.store(true, Ordering::Release);
    owner.changed.notify_all();
    worker.join().unwrap();
  }

  #[test]
  fn old_process_epochs_and_empty_failures_cannot_refresh_the_cached_snapshot() {
    let dir = Directory::new();
    let owner = dir.owner();
    owner.publish(Some(Frame::fixture(0, 0)), None).unwrap();
    demand(&owner, ProcessDemand::Details);
    let epoch = owner.data.lock().unwrap().process_epoch;
    let snapshot =
      ProcSampler::snapshot(&[fake_process()], Instant::now() - Duration::from_secs(5));
    let frame = Arc::new(ProcessFrame::fixture(snapshot));
    owner.publish_checked(None, Some(frame.clone()), Some((epoch, false))).unwrap();
    demand(&owner, ProcessDemand::None);
    demand(&owner, ProcessDemand::Details);
    let next =
      Arc::new(ProcessFrame::fixture(ProcSampler::snapshot(&[fake_process()], Instant::now())));
    owner.publish_checked(None, Some(next), Some((epoch, false))).unwrap();
    assert!(Arc::ptr_eq(owner.data.lock().unwrap().processes.as_ref().unwrap(), &frame));
    let worker = {
      let owner = owner.clone();
      thread::spawn(move || sample_processes_with(&owner, || FakeReader(Vec::new)))
    };
    until(|| owner.data.lock().unwrap().process_state == ProcessState::Failed);
    assert!(Arc::ptr_eq(owner.data.lock().unwrap().processes.as_ref().unwrap(), &frame));
    assert!(
      owner.data.lock().unwrap().processes.as_ref().unwrap().snapshot.sampled.elapsed()
        >= Duration::from_secs(5)
    );
    owner.stopped.store(true, Ordering::Release);
    owner.changed.notify_all();
    worker.join().unwrap();
  }

  struct Directory(PathBuf);
  impl Directory {
    fn new() -> Self {
      let path = PathBuf::from(format!("/private/tmp/mm-source-{}", unique()));
      fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
      Self(path)
    }
    fn runtime(&self) -> Runtime {
      Runtime::at(self.0.clone()).unwrap()
    }
    fn owner(&self) -> Arc<Owner> {
      Arc::new(Owner::new(self.runtime().owner_lock().unwrap(), 1000))
    }
  }
  impl Drop for Directory {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }
  fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !check() {
      assert!(Instant::now() < deadline);
      thread::sleep(Duration::from_millis(5));
    }
  }

  fn revisioned_cadence(requested: u32, effective: u32, generation: u64) -> CadenceState {
    serde_json::from_value(serde_json::json!({
      "requested_ms": requested, "effective_ms": effective,
      "pending": requested != effective, "generation": generation,
    }))
    .unwrap()
  }

  #[test]
  fn queued_frames_and_heartbeats_cannot_roll_back_an_acknowledged_adjustment() {
    let dir = Directory::new();
    let runtime = dir.runtime();
    let listener = UnixListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let (release, next) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
      let (mut subscription, _) = listener.accept().unwrap();
      let _ = read_packet(&mut subscription).unwrap();
      let mut frame = Frame::fixture(1, 1);
      frame.cadence = revisioned_cadence(250, 250, 0);
      write_json(
        &mut subscription,
        b'H',
        &Welcome { protocol: PROTOCOL, session: frame.session.clone(), cadence: frame.cadence },
      )
      .unwrap();
      write_json(&mut subscription, b'F', &frame).unwrap();
      let (mut command, _) = listener.accept().unwrap();
      let (_, bytes) = read_packet(&mut command).unwrap();
      assert!(matches!(
        serde_json::from_slice::<Request>(&bytes).unwrap(),
        Request::Adjust { adjustment, .. } if adjustment == "increase"
      ));
      write_json(&mut command, b'C', &revisioned_cadence(500, 250, 1)).unwrap();
      next.recv_timeout(IO_TIMEOUT).unwrap();
      frame.revision = 2;
      write_json(&mut subscription, b'F', &frame).unwrap();
      next.recv_timeout(IO_TIMEOUT).unwrap();
      write_json(&mut subscription, b'C', &frame.cadence).unwrap();
      frame.revision = 3; // Observing this frame also proves the preceding C was consumed.
      write_json(&mut subscription, b'F', &frame).unwrap();
      let (mut command, _) = listener.accept().unwrap();
      let _ = read_packet(&mut command).unwrap();
      frame.revision = 4;
      frame.cadence = revisioned_cadence(1000, 250, 3);
      write_json(&mut subscription, b'F', &frame).unwrap();
      next.recv_timeout(IO_TIMEOUT).unwrap();
      // Another consumer's later change arrived before this control connection's older ACK.
      write_json(&mut command, b'C', &revisioned_cadence(750, 250, 2)).unwrap();
      let _ = subscription.read(&mut [0]);
    });
    let client = Arc::new(Client::at(runtime, None, ProcessDemand::Details, 1000).unwrap());
    let first = client.wait_after(None, IO_TIMEOUT).unwrap().unwrap();
    assert_eq!(client.adjust("increase").unwrap().requested_ms, 500);
    assert_eq!(client.latest().unwrap().cadence.requested_ms, 500, "normalize cached F too");
    release.send(()).unwrap();
    let frame =
      client.wait_after(Some((&first.session, first.revision)), IO_TIMEOUT).unwrap().unwrap();
    assert_eq!(frame.cadence.requested_ms, 500, "a queued F must retain the accepted cadence");
    assert_eq!(client.cadence().requested_ms, 500);
    release.send(()).unwrap();
    let frame =
      client.wait_after(Some((&frame.session, frame.revision)), IO_TIMEOUT).unwrap().unwrap();
    assert_eq!(frame.cadence.requested_ms, 500, "an older C must not roll back a later frame");
    assert_eq!(replacement_interval(client.cadence().requested_ms, Some(5000), 1000), 500);
    let adjusting = Arc::clone(&client);
    let adjustment = thread::spawn(move || adjusting.adjust("increase"));
    until(|| client.latest().unwrap().revision == 4);
    release.send(()).unwrap();
    assert_eq!(adjustment.join().unwrap().unwrap().requested_ms, 1000);
    assert_eq!(client.cadence().requested_ms, 1000);
    drop(client);
    server.join().unwrap();
  }

  #[test]
  fn welcome_resets_cadence_only_for_a_new_session_and_legacy_owners_still_decode() {
    let frame = Frame::fixture(1, 1);
    let mut data = ClientData {
      frame: Some(Arc::new(frame)),
      state: ConnectionState::Live,
      cadence: revisioned_cadence(500, 250, 3),
      session: "fixture".into(),
      error: String::new(),
      socket: None,
    };
    data.welcome("fixture", revisioned_cadence(250, 250, 0));
    assert_eq!(data.cadence.requested_ms, 500);
    assert!(!data.accept_cadence("fixture", revisioned_cadence(250, 250, 2)));
    assert!(data.accept_cadence("fixture", revisioned_cadence(500, 500, 4)));
    assert_eq!(data.frame.as_ref().unwrap().cadence.effective_ms, 500);
    data.welcome("replacement", revisioned_cadence(750, 750, 0));
    assert_eq!((data.cadence.requested_ms, data.cadence.generation), (750, 0));
    assert!(!data.accept_cadence("fixture", revisioned_cadence(250, 250, 100)));
    let legacy: CadenceState = serde_json::from_value(serde_json::json!({
      "requested_ms": 1000, "effective_ms": 750, "pending": true,
    }))
    .unwrap();
    assert_eq!(legacy.generation, 0);
    assert!(data.accept_cadence("replacement", legacy));
    assert_eq!(data.cadence.requested_ms, 1000);
  }

  #[test]
  fn an_old_session_control_ack_cannot_change_a_reconnected_clients_cadence() {
    let dir = Directory::new();
    let runtime = dir.runtime();
    let listener = UnixListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let (release, next) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
      let (mut subscription, _) = listener.accept().unwrap();
      let _ = read_packet(&mut subscription).unwrap();
      let mut frame = Frame::fixture(1, 1);
      frame.cadence = revisioned_cadence(250, 250, 2);
      write_json(
        &mut subscription,
        b'H',
        &Welcome { protocol: PROTOCOL, session: frame.session.clone(), cadence: frame.cadence },
      )
      .unwrap();
      write_json(&mut subscription, b'F', &frame).unwrap();
      let (mut command, _) = listener.accept().unwrap();
      let _ = read_packet(&mut command).unwrap();
      drop(subscription);
      let (mut replacement, _) = listener.accept().unwrap();
      let _ = read_packet(&mut replacement).unwrap();
      frame.session = "replacement".into();
      frame.cadence = revisioned_cadence(750, 750, 0);
      write_json(
        &mut replacement,
        b'H',
        &Welcome { protocol: PROTOCOL, session: frame.session.clone(), cadence: frame.cadence },
      )
      .unwrap();
      write_json(&mut replacement, b'F', &frame).unwrap();
      next.recv_timeout(IO_TIMEOUT).unwrap();
      write_json(&mut command, b'C', &revisioned_cadence(500, 250, 3)).unwrap();
      let _ = replacement.read(&mut [0]);
    });
    let client = Arc::new(Client::at(runtime, None, ProcessDemand::Details, 1000).unwrap());
    assert!(client.wait_after(None, IO_TIMEOUT).unwrap().is_some());
    let adjusting = Arc::clone(&client);
    let adjustment = thread::spawn(move || adjusting.adjust("increase"));
    until(|| client.latest().is_some_and(|frame| frame.session == "replacement"));
    release.send(()).unwrap();
    assert_eq!(adjustment.join().unwrap().unwrap_err().kind(), io::ErrorKind::NotConnected);
    assert_eq!((client.cadence().requested_ms, client.cadence().generation), (750, 0));
    assert_eq!(client.latest().unwrap().cadence.requested_ms, 750);
    drop(client);
    server.join().unwrap();
  }

  fn subscribe(owner: &Arc<Owner>, id: &str, initial_ms: Option<u32>) -> UnixStream {
    let (mut client, server) = UnixStream::pair().unwrap();
    client.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    let owner = Arc::clone(owner);
    thread::spawn(move || {
      let _ = owner_connection(server, owner);
    });
    write_json(
      &mut client,
      b'Q',
      &Request::Subscribe {
        protocol: PROTOCOL,
        id: id.into(),
        initial_ms,
        demand: ProcessDemand::Details,
      },
    )
    .unwrap();
    assert_eq!(read_packet(&mut client).unwrap().0, b'H');
    client
  }
  fn control(owner: &Arc<Owner>, request: &Request) -> io::Result<CadenceState> {
    let (mut client, server) = UnixStream::pair()?;
    client.set_read_timeout(Some(IO_TIMEOUT))?;
    let owner = Arc::clone(owner);
    thread::spawn(move || {
      let _ = owner_connection(server, owner);
    });
    write_json(&mut client, b'Q', request)?;
    let (kind, bytes) = read_packet(&mut client)?;
    assert_eq!(kind, b'C');
    Ok(serde_json::from_slice(&bytes)?)
  }

  #[test]
  fn runtime_paths_ignore_tmpdir_and_reject_unsafe_objects() {
    assert_eq!(Runtime::default_directory(501), PathBuf::from("/private/tmp/macmon-501"));
    assert!(Runtime::at(PathBuf::from("relative")).is_err());
    let dir = Directory::new();
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Runtime::at(dir.0.clone()).is_err());
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o700)).unwrap();
    let link = dir.0.with_extension("link");
    symlink(&dir.0, &link).unwrap();
    assert!(Runtime::at(link.clone()).is_err());
    fs::remove_file(link).unwrap();
    fs::write(dir.runtime().socket(), b"not a socket").unwrap();
    assert!(dir.runtime().connect().is_err());
    fs::remove_file(dir.runtime().socket()).unwrap();
    symlink("/dev/null", dir.0.join("collector.lock")).unwrap();
    assert!(dir.runtime().owner_lock().is_err());
  }

  #[test]
  fn only_one_owner_can_hold_the_sampling_lock() {
    let dir = Directory::new();
    let lock = dir.runtime().owner_lock().unwrap();
    assert_eq!(dir.runtime().owner_lock().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    drop(lock);
    // Parallel tests can fork while the descriptor is open; CLOEXEC closes the
    // child's inherited copy at exec, briefly extending the same exclusive lock.
    until(|| match dir.runtime().owner_lock() {
      Ok(_) => true,
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
      Err(error) => panic!("released sampling lock cannot be acquired: {error}"),
    });
  }

  #[test]
  fn cached_samples_age_and_oversized_frames_fail_before_body_allocation() {
    let dir = Directory::new();
    let owner = dir.owner();
    let mut frame = Frame::fixture(0, 0);
    frame.sampled -= Duration::from_millis(500);
    owner.publish(Some(frame), None).unwrap();
    thread::sleep(Duration::from_millis(25));
    let packet = owner.data.lock().unwrap().latest.clone().unwrap();
    let (mut a, mut b) = UnixStream::pair().unwrap();
    write_packet(&mut a, b'F', &packet.bytes).unwrap();
    let (_, bytes) = read_packet(&mut b).unwrap();
    let frame: Frame = serde_json::from_slice(&bytes).unwrap();
    let sampled = frame.sampled;
    assert!(sampled.elapsed() >= Duration::from_millis(525));
    a.write_all(b"F").unwrap();
    a.write_all(&((MAX_FRAME + 1) as u32).to_be_bytes()).unwrap();
    assert_eq!(read_packet(&mut b).unwrap_err().kind(), io::ErrorKind::InvalidData);
  }

  #[test]
  fn absolute_sample_age_includes_slow_encoding_queue_and_decoding_without_double_counting() {
    #[derive(Serialize, Deserialize)]
    struct Times {
      #[serde(with = "instant")]
      first: Instant,
      #[serde(with = "delay")]
      pause: (),
      #[serde(with = "instant")]
      last: Instant,
    }
    mod delay {
      pub fn serialize<S: serde::Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
        std::thread::sleep(std::time::Duration::from_millis(25));
        serializer.serialize_unit()
      }
      pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
      ) -> Result<(), D::Error> {
        <() as serde::Deserialize>::deserialize(deserializer)?;
        std::thread::sleep(std::time::Duration::from_millis(25));
        Ok(())
      }
    }
    let sampled = Instant::now() - Duration::from_millis(100);
    let bytes = serde_json::to_vec(&Times { first: sampled, pause: (), last: sampled }).unwrap();
    thread::sleep(Duration::from_millis(40));
    let decoded: Times = serde_json::from_slice(&bytes).unwrap();
    for instant in [decoded.first, decoded.last] {
      assert!(instant.elapsed() >= Duration::from_millis(190));
      assert!(
        instant.duration_since(sampled).max(sampled.duration_since(instant))
          < Duration::from_millis(2)
      );
    }
    assert!(
      decoded.first.duration_since(decoded.last).max(decoded.last.duration_since(decoded.first))
        < Duration::from_millis(2)
    );
  }

  #[test]
  fn independent_process_publication_preserves_system_identity_and_age() {
    let dir = Directory::new();
    let owner = dir.owner();
    owner.publish(Some(Frame::fixture(0, 0)), None).unwrap();
    let sampled = owner.data.lock().unwrap().frame.as_ref().unwrap().sampled;
    let processes = ProcessFrame::fixture(ProcessSnapshot {
      rows: Vec::new(),
      groups: Vec::new(),
      sampled: sampled - Duration::from_secs(2),
    });
    owner.publish(None, Some(Arc::new(processes))).unwrap();
    let data = owner.data.lock().unwrap();
    assert_eq!((data.sequence, data.revision), (1, 2));
    let frame = data.frame.as_ref().unwrap();
    assert_eq!(frame.sampled, sampled);
    assert_eq!(
      frame.processes.as_ref().unwrap().snapshot.sampled,
      sampled - Duration::from_secs(2)
    );
    assert_eq!(data.latest.as_ref().unwrap().revision, 2);
  }

  #[test]
  fn accepted_nonblocking_stream_does_not_look_like_a_disconnected_consumer() {
    let dir = Directory::new();
    let owner = dir.owner();
    let (mut client, server) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    client.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    write_json(
      &mut client,
      b'Q',
      &Request::Subscribe {
        protocol: PROTOCOL,
        id: "accepted".into(),
        initial_ms: None,
        demand: ProcessDemand::Details,
      },
    )
    .unwrap();
    let owned = Arc::clone(&owner);
    let connection = thread::spawn(move || owner_connection(server, owned));
    assert_eq!(read_packet(&mut client).unwrap().0, b'H');
    thread::sleep(Duration::from_millis(30));
    assert!(owner.data.lock().unwrap().leases.contains_key("accepted"));
    drop(client);
    until(|| owner.data.lock().unwrap().leases.is_empty());
    let _ = connection.join().unwrap();
  }

  #[test]
  fn reconnect_generation_prevents_old_connection_from_removing_new_lease() {
    let dir = Directory::new();
    let owner = dir.owner();
    let first = subscribe(&owner, "same-client", None);
    let old = owner.data.lock().unwrap().leases["same-client"].1.clone();
    let second = subscribe(&owner, "same-client", None);
    let new = owner.data.lock().unwrap().leases["same-client"].1.clone();
    assert_ne!(old, new);
    first.shutdown(Shutdown::Both).unwrap();
    drop(first);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(owner.data.lock().unwrap().leases["same-client"].1, new);
    second.shutdown(Shutdown::Both).unwrap();
    drop(second);
    until(|| owner.data.lock().unwrap().leases.is_empty());
  }

  #[test]
  fn subscriptions_are_bounded_and_adjustments_are_atomic_owner_relative() {
    let dir = Directory::new();
    let owner = dir.owner();
    let clients: Vec<_> =
      (0..MAX_SUBSCRIBERS).map(|i| subscribe(&owner, &i.to_string(), None)).collect();
    let (mut client, server) = UnixStream::pair().unwrap();
    let owned = Arc::clone(&owner);
    let rejected = thread::spawn(move || owner_connection(server, owned));
    write_json(
      &mut client,
      b'Q',
      &Request::Subscribe {
        protocol: PROTOCOL,
        id: "overflow".into(),
        initial_ms: None,
        demand: ProcessDemand::Details,
      },
    )
    .unwrap();
    assert!(rejected.join().unwrap().is_err());
    assert!(read_packet(&mut client).is_err());
    let request = || Request::Adjust {
      protocol: PROTOCOL,
      session: owner.session.clone(),
      adjustment: "increase".into(),
    };
    assert_eq!(control(&owner, &request()).unwrap().requested_ms, 1250);
    assert_eq!(control(&owner, &request()).unwrap().requested_ms, 1500);
    assert!(
      control(
        &owner,
        &Request::Adjust {
          protocol: PROTOCOL,
          session: "old-session".into(),
          adjustment: "increase".into()
        }
      )
      .is_err()
    );
    assert_eq!(owner.data.lock().unwrap().cadence.state().effective_ms, 1000);
    drop(clients);
    until(|| owner.data.lock().unwrap().leases.is_empty());
  }

  #[test]
  fn replacement_seed_prefers_shared_cadence_then_explicit_cli_then_default() {
    assert_eq!(replacement_interval(500, Some(5000), 1000), 500);
    assert_eq!(replacement_interval(0, Some(5000), 1000), 5000);
    assert_eq!(replacement_interval(0, None, 1000), 1000);
  }

  #[test]
  fn private_socket_birth_child() {
    let Some(path) = std::env::var_os("MACMON_TEST_PRIVATE_BIND") else { return };
    unsafe {
      libc::umask(0o022);
    }
    let runtime = Runtime::at(path.into()).unwrap();
    let _listener = bind_private_socket(&runtime).unwrap();
    // Check birth permissions before run() gets a chance to apply chmod.
    assert_eq!(fs::symlink_metadata(runtime.socket()).unwrap().mode() & 0o077, 0);
  }

  #[test]
  fn socket_is_private_from_bind_even_when_the_callers_umask_is_public() {
    let directory = Directory::new();
    let output = Command::new(std::env::current_exe().unwrap())
      .args(["--exact", "source::tests::private_socket_birth_child", "--nocapture"])
      .env("MACMON_TEST_PRIVATE_BIND", &directory.0)
      .output()
      .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
  }

  #[test]
  fn new_consumers_adopt_existing_cadence_and_explicit_intervals_are_active() {
    let dir = Directory::new();
    let owner = dir.owner();
    let first = subscribe(&owner, "first", Some(5000));
    let second = subscribe(&owner, "second", None);
    assert_eq!(owner.data.lock().unwrap().cadence.state().requested_ms, 5000);
    let third = subscribe(&owner, "third", Some(750));
    assert_eq!(owner.data.lock().unwrap().cadence.state().requested_ms, 750);
    drop((first, second, third));
  }

  #[test]
  fn demand_changed_during_handshake_is_sent_after_the_actual_subscribed_value() {
    let dir = Directory::new();
    let runtime = dir.runtime();
    let listener = UnixListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let (arrived, ready) = std::sync::mpsc::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
      let (mut subscription, _) = listener.accept().unwrap();
      let (_, bytes) = read_packet(&mut subscription).unwrap();
      assert!(matches!(
        serde_json::from_slice::<Request>(&bytes).unwrap(),
        Request::Subscribe { demand: ProcessDemand::None, .. }
      ));
      arrived.send(()).unwrap();
      resumed.recv_timeout(IO_TIMEOUT).unwrap();
      let frame = Frame::fixture(1, 1);
      write_json(
        &mut subscription,
        b'H',
        &Welcome { protocol: PROTOCOL, session: frame.session.clone(), cadence: frame.cadence },
      )
      .unwrap();
      let (mut command, _) = listener.accept().unwrap();
      let (_, bytes) = read_packet(&mut command).unwrap();
      assert!(matches!(
        serde_json::from_slice::<Request>(&bytes).unwrap(),
        Request::Demand { demand: ProcessDemand::Details, .. }
      ));
      write_json(&mut command, b'C', &frame.cadence).unwrap();
      write_json(&mut subscription, b'F', &frame).unwrap();
      let _ = subscription.read(&mut [0]);
    });
    let client = Client::at(runtime, None, ProcessDemand::None, 1000).unwrap();
    ready.recv_timeout(IO_TIMEOUT).unwrap();
    let start = Instant::now();
    client.set_demand(ProcessDemand::Details).unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    resume.send(()).unwrap();
    assert!(client.wait_after(None, IO_TIMEOUT).unwrap().is_some());
    drop(client);
    server.join().unwrap();
  }

  #[test]
  fn tmpdir_child_connects_to_the_same_canonical_namespace() {
    let Ok(label) = std::env::var("MACMON_TEST_RUNTIME_LABEL") else { return };
    let runtime = Runtime::at(Runtime::default_directory(label.parse().unwrap())).unwrap();
    let mut stream = runtime.connect().unwrap();
    write_json(
      &mut stream,
      b'Q',
      &Request::Subscribe {
        protocol: PROTOCOL,
        id: unique(),
        initial_ms: None,
        demand: ProcessDemand::None,
      },
    )
    .unwrap();
    let (_, bytes) = read_packet(&mut stream).unwrap();
    let welcome: Welcome = serde_json::from_slice(&bytes).unwrap();
    println!("canonical-session={}", welcome.session);
  }

  #[test]
  fn consumers_with_different_tmpdir_values_share_one_session() {
    // A synthetic UID label keeps this regression away from the real user's namespace.
    let label = (1_u32 << 31) + std::process::id();
    let directory = Directory(Runtime::default_directory(label));
    assert!(!directory.0.exists());
    let runtime = directory.runtime();
    let owner = directory.owner();
    let listener = UnixListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let expected = format!("canonical-session={}", owner.session);
    let accept = thread::spawn(move || {
      for _ in 0..2 {
        let (stream, _) = listener.accept().unwrap();
        let owner = Arc::clone(&owner);
        thread::spawn(move || {
          let _ = owner_connection(stream, owner);
        });
      }
    });
    for tmpdir in ["/private/tmp", "/private/tmp/irrelevant-launchd-tmpdir"] {
      let output = Command::new(std::env::current_exe().unwrap())
        .args([
          "--exact",
          "source::tests::tmpdir_child_connects_to_the_same_canonical_namespace",
          "--nocapture",
        ])
        .env("TMPDIR", tmpdir)
        .env("MACMON_TEST_RUNTIME_LABEL", label.to_string())
        .output()
        .unwrap();
      assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
      assert!(String::from_utf8_lossy(&output.stdout).contains(&expected));
    }
    accept.join().unwrap();
  }

  #[test]
  #[ignore = "rootless hardware probe; set MACMON_TEST_COLLECTOR_BIN to the matching application binary"]
  fn shared_sampler_rootless_fixture() {
    let dir = Directory::new();
    let first = Client::at(dir.runtime(), Some(250), ProcessDemand::None, 1000).unwrap();
    let a = first.wait_after(None, Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(a.process_state, ProcessState::Disabled);
    assert!(a.processes.is_none());
    first.set_demand(ProcessDemand::Summary).unwrap();
    until(|| {
      first.latest().is_some_and(|frame| {
        frame.processes.is_some() && frame.process_demand == ProcessDemand::Summary
      })
    });
    let summary = first.latest().unwrap();
    assert_eq!(summary.process_interval_ms, 5000);
    assert_eq!(summary.process_state, ProcessState::Warming);
    assert!(summary.processes.as_ref().unwrap().rows.iter().all(|row| !row.meta.known.0));
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(
      first.latest().unwrap().processes.as_ref().unwrap().sequence,
      summary.processes.as_ref().unwrap().sequence
    );
    first.set_demand(ProcessDemand::Details).unwrap();
    until(|| {
      first.latest().is_some_and(|frame| {
        frame.process_state == ProcessState::Ready && frame.process_interval_ms == 1000
      })
    });
    let second = Client::at(dir.runtime(), None, ProcessDemand::Details, 1000).unwrap();
    let b = second.wait_after(None, Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!((a.session.as_str(), a.collector_pid), (b.session.as_str(), b.collector_pid));
    assert_eq!(first.adjust("increase").unwrap().requested_ms, 500);
    until(|| second.cadence().requested_ms == 500);
    let pid = a.collector_pid;
    drop(first);
    let next =
      second.wait_after(Some((&b.session, b.revision)), Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(next.collector_pid, pid);
    // Only this private namespace's observed owner is signalled.
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    let replacement = loop {
      assert!(Instant::now() < deadline);
      if let Some(frame) = second.latest()
        && frame.session != a.session
      {
        break frame;
      }
      thread::sleep(Duration::from_millis(20));
    };
    assert_ne!(replacement.collector_pid, pid);
    assert_eq!(
      replacement.cadence.requested_ms, 500,
      "recovery preserves the last shared interval"
    );
    until(|| second.latest().is_some_and(|frame| frame.process_state == ProcessState::Ready));
    let before_pause = second.latest().unwrap();
    second.set_demand(ProcessDemand::None).unwrap();
    until(|| second.latest().is_some_and(|frame| frame.process_state == ProcessState::Disabled));
    let paused = second.latest().unwrap();
    let cached =
      paused.processes.as_ref().map(|processes| (processes.sequence, processes.snapshot.sampled));
    thread::sleep(Duration::from_millis(1200));
    let after = second.latest().unwrap();
    assert_eq!(after.process_interval_ms, 0);
    assert_eq!(
      after.processes.as_ref().map(|processes| processes.sequence),
      cached.map(|value| value.0)
    );
    if let Some((_, sampled)) = cached {
      assert!(
        after.processes.as_ref().unwrap().snapshot.sampled.elapsed()
          >= sampled.elapsed().saturating_sub(Duration::from_millis(2))
      );
    }
    second.set_demand(ProcessDemand::Details).unwrap();
    until(|| {
      second.latest().is_some_and(|frame| {
        frame
          .processes
          .as_ref()
          .is_some_and(|processes| Some(processes.sequence) != cached.map(|value| value.0))
      })
    });
    let resumed = second.latest().unwrap();
    assert!(resumed.processes.as_ref().unwrap().rows.iter().all(|row| !row.meta.known.0));
    for row in &resumed.processes.as_ref().unwrap().rows {
      if let Some(before) = before_pause.processes.as_ref().unwrap().rows.iter().find(|before| {
        before.pid == row.pid
          && before.meta.identity.is_some()
          && before.meta.identity == row.meta.identity
      }) {
        assert_eq!(before.meta.instance, row.meta.instance);
        if before.meta.app.is_some() && row.meta.app.is_some() {
          assert_eq!(before.meta.app, row.meta.app);
        }
      }
    }
    let replacement_pid = replacement.collector_pid;
    drop(second);
    until(|| !dir.runtime().socket().exists());
    until(|| unsafe { libc::kill(replacement_pid as i32, 0) } != 0);
    eprintln!(
      "shared rootless probe: uid={} runtime={} initial_session={} replacement_session={} old_pid={pid} replacement_pid={replacement_pid}; shared session, cadence, recovery and last-lease exit passed",
      unsafe { libc::geteuid() },
      dir.0.display(),
      a.session,
      replacement.session
    );
  }
}
