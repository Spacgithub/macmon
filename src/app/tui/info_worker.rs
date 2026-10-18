//! One selected PID's details; no descriptor reads run on the terminal event thread.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::fds::{self, OpenFile};
use crate::proc_groups::{self, Identity};
use crate::procs;

pub(super) const INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
  pub pid: i32,
  pub started: Option<(u64, u64)>,
  pub instance: String,
  pub identity: Option<Identity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
  pub generation: u64,
  pub target: Target,
}

#[derive(Debug)]
pub(super) struct Completion {
  pub request: Request,
  pub sampled: Instant,
  pub files: Result<Vec<OpenFile>, i32>,
}

trait Reader {
  fn identity(&mut self, pid: i32) -> Option<Identity>;
  fn started(&mut self, pid: i32) -> Option<(u64, u64)>;
  fn files(&mut self, pid: i32) -> Result<Vec<OpenFile>, i32>;
}

struct Native;

impl Reader for Native {
  fn identity(&mut self, pid: i32) -> Option<Identity> {
    proc_groups::identity(pid)
  }

  fn started(&mut self, pid: i32) -> Option<(u64, u64)> {
    let info = procs::bsd_info(pid)?;
    (info.pbi_status != libc::SZOMB).then_some((info.pbi_start_tvsec, info.pbi_start_tvusec))
  }

  fn files(&mut self, pid: i32) -> Result<Vec<OpenFile>, i32> {
    fds::open_files(pid)
  }
}

fn read(reader: &mut impl Reader, target: &Target) -> Result<Vec<OpenFile>, i32> {
  if target.pid <= 0 {
    return Err(libc::ESRCH);
  }
  let expected = target.started.ok_or(libc::EPERM)?;
  let before = reader.identity(target.pid);
  if target.identity.is_some() && before != target.identity
    || reader.started(target.pid) != Some(expected)
  {
    return Err(libc::ESRCH);
  }
  let files = reader.files(target.pid)?;
  if before != reader.identity(target.pid) || reader.started(target.pid) != Some(expected) {
    return Err(libc::ESRCH);
  }
  Ok(files)
}

#[derive(Debug)]
struct State {
  request: Option<Request>,
  revision: u64,
  interval: Duration,
  completed: Option<Instant>,
  due: Instant,
  stopped: bool,
}

#[derive(Debug)]
pub(super) struct Worker {
  state: Arc<(Mutex<State>, Condvar)>,
}

impl Worker {
  pub(super) fn new(publish: impl FnMut(Completion) -> bool + Send + 'static) -> Self {
    Self::spawn(|target| read(&mut Native, target), publish)
  }

  fn spawn(
    mut read: impl FnMut(&Target) -> Result<Vec<OpenFile>, i32> + Send + 'static,
    mut publish: impl FnMut(Completion) -> bool + Send + 'static,
  ) -> Self {
    let state = Arc::new((
      Mutex::new(State {
        request: None,
        revision: 0,
        interval: INTERVAL,
        completed: None,
        due: Instant::now(),
        stopped: false,
      }),
      Condvar::new(),
    ));
    let shared = state.clone();
    std::thread::spawn(move || {
      let (lock, changed) = &*shared;
      loop {
        let (request, revision) = {
          let mut state = lock.lock().unwrap();
          loop {
            if state.stopped {
              return;
            }
            if let Some(request) = &state.request {
              let wait = state.due.saturating_duration_since(Instant::now());
              if wait.is_zero() {
                break (request.clone(), state.revision);
              }
              state = changed.wait_timeout(state, wait).unwrap().0;
            } else {
              state = changed.wait(state).unwrap();
            }
          }
        };
        let sampled = Instant::now();
        let files = read(&request.target);
        let mut state = lock.lock().unwrap();
        if state.stopped {
          return;
        }
        if state.revision != revision {
          continue;
        }
        let completed = Instant::now();
        state.completed = Some(completed);
        state.due = completed + state.interval;
        drop(state);
        if !publish(Completion { request, sampled, files }) {
          return;
        }
      }
    });
    Self { state }
  }

  /// Latest target wins. Repeated list samples do not restart the detail deadline.
  pub(super) fn set(&self, request: Option<Request>, interval: Duration) {
    let (lock, changed) = &*self.state;
    let mut state = lock.lock().unwrap();
    if state.request != request {
      state.request = request;
      state.revision = state.revision.wrapping_add(1);
      state.completed = None;
      state.due = Instant::now();
    } else if state.interval == interval {
      return;
    } else if let Some(completed) = state.completed {
      state.due = completed + interval;
    }
    state.interval = interval;
    changed.notify_all();
  }
}

impl Drop for Worker {
  fn drop(&mut self) {
    let (lock, changed) = &*self.state;
    lock.lock().unwrap().stopped = true;
    changed.notify_all(); // Never join a possibly slow native read on terminal exit.
  }
}

#[cfg(test)]
mod tests {
  use std::collections::VecDeque;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::mpsc;

  use super::*;

  fn target() -> Target {
    Target {
      pid: 42,
      started: Some((10, 20)),
      instance: "opaque-one".into(),
      identity: Some(Identity { unique: 9, version: 3 }),
    }
  }

  fn request(generation: u64) -> Request {
    Request { generation, target: target() }
  }

  struct Fake {
    identities: VecDeque<Option<Identity>>,
    starts: VecDeque<Option<(u64, u64)>>,
    files: usize,
  }

  impl Fake {
    fn new() -> Self {
      Self {
        identities: [target().identity; 2].into(),
        starts: [target().started; 2].into(),
        files: 0,
      }
    }
  }

  impl Reader for Fake {
    fn identity(&mut self, _: i32) -> Option<Identity> {
      self.identities.pop_front().unwrap()
    }
    fn started(&mut self, _: i32) -> Option<(u64, u64)> {
      self.starts.pop_front().unwrap()
    }
    fn files(&mut self, _: i32) -> Result<Vec<OpenFile>, i32> {
      self.files += 1;
      Ok(Vec::new())
    }
  }

  #[test]
  fn validates_expected_lifetime_before_reading_any_descriptors() {
    let mut fake = Fake::new();
    assert_eq!(read(&mut fake, &Target { pid: 0, ..target() }).unwrap_err(), libc::ESRCH);
    assert_eq!(fake.files, 0);
    assert_eq!(read(&mut fake, &Target { started: None, ..target() }).unwrap_err(), libc::EPERM);
    assert_eq!(fake.files, 0);
    for identity in
      [None, Some(Identity { unique: 99, version: 3 }), Some(Identity { unique: 9, version: 4 })]
    {
      let mut fake = Fake::new();
      fake.identities[0] = identity;
      assert_eq!(read(&mut fake, &target()).unwrap_err(), libc::ESRCH);
      assert_eq!(fake.files, 0);
    }
    let mut fake = Fake::new();
    fake.starts[0] = Some((99, 0));
    assert_eq!(read(&mut fake, &target()).unwrap_err(), libc::ESRCH);
    assert_eq!(fake.files, 0);
  }

  #[test]
  fn rejects_exec_and_pid_reuse_during_reads_but_keeps_bsd_fallback() {
    for changed in [Identity { unique: 99, version: 3 }, Identity { unique: 9, version: 4 }] {
      let mut fake = Fake::new();
      fake.identities[1] = Some(changed);
      assert_eq!(read(&mut fake, &target()).unwrap_err(), libc::ESRCH);
      assert_eq!(fake.files, 1);
    }
    let mut fake = Fake::new();
    fake.starts[1] = None;
    assert_eq!(read(&mut fake, &target()).unwrap_err(), libc::ESRCH);
    let mut fake = Fake::new();
    fake.identities = [None, None].into();
    assert!(read(&mut fake, &Target { identity: None, ..target() }).is_ok());
  }

  #[test]
  fn idle_worker_reads_nothing_and_repeated_targets_do_not_rescan() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn(
      move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
      },
      move |value| tx.send(value).is_ok(),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    worker.set(Some(request(1)), Duration::from_secs(60));
    let completed = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(completed.request.generation, 1);
    assert!(completed.files.is_ok());
    assert!(completed.sampled <= Instant::now());
    for _ in 0..10 {
      worker.set(Some(request(1)), Duration::from_secs(60));
    }
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    worker.set(Some(request(2)), Duration::from_secs(60));
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().request.generation, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    worker.set(None, INTERVAL);
  }

  #[test]
  fn slow_reads_cannot_block_new_requests_and_only_the_latest_result_is_published() {
    let (began_tx, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn(
      move |target| {
        began_tx.send(target.clone()).unwrap();
        gate.recv().unwrap();
        Ok(Vec::new())
      },
      move |value| tx.send(value).is_ok(),
    );
    worker.set(Some(request(1)), INTERVAL);
    began.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.set(Some(request(2)), INTERVAL);
    worker.set(None, INTERVAL);
    worker.set(Some(request(3)), INTERVAL);
    release.send(()).unwrap();
    began.recv_timeout(Duration::from_secs(2)).unwrap();
    release.send(()).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().request.generation, 3);
    assert!(rx.try_recv().is_err());
    worker.set(None, INTERVAL);
  }

  #[test]
  fn interval_changes_recompute_the_deadline_without_restarting_a_read() {
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn(|_| Ok(Vec::new()), move |value| tx.send(value).is_ok());
    worker.set(Some(request(1)), INTERVAL);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let completed = worker.state.0.lock().unwrap().completed.unwrap();
    for interval in [Duration::from_secs(10), INTERVAL] {
      worker.set(Some(request(1)), interval);
      let state = worker.state.0.lock().unwrap();
      assert_eq!(state.due.duration_since(completed), interval);
      assert_eq!(state.revision, 1);
    }
    worker.set(None, INTERVAL);
  }

  #[test]
  fn shutdown_does_not_wait_for_native_io_and_discards_its_completion() {
    let (began_tx, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let worker = Worker::spawn(
      move |_| {
        began_tx.send(()).unwrap();
        gate.recv().unwrap();
        Ok(Vec::new())
      },
      move |value| tx.send(value).is_ok(),
    );
    worker.set(Some(request(1)), INTERVAL);
    began.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(worker);
    release.send(()).unwrap();
    assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());
  }
}
