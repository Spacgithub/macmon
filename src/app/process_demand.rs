//! Ephemeral browser demand. Only a server-issued live lease opts out of legacy collection.

use std::collections::HashMap;
use std::io::{self, Read};
use std::time::{Duration, Instant};

use crate::source::ProcessDemand;

pub(crate) const TTL_MS: u64 = 15_000;
const LIMIT: usize = 32;

pub(crate) fn interval_ms(demand: ProcessDemand, system_ms: u32) -> Option<u32> {
  match demand {
    ProcessDemand::None => None,
    ProcessDemand::Summary => Some(system_ms.max(5_000)),
    ProcessDemand::Details => Some(system_ms.max(1_000)),
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
  Invalid,
  Expired,
  Capacity,
  Unavailable,
}

#[derive(Debug)]
struct Lease {
  demand: ProcessDemand,
  expires: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct Leases {
  clients: HashMap<String, Lease>,
  legacy: Option<Instant>,
}

fn valid(token: &str) -> bool {
  token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn token() -> io::Result<String> {
  let mut bytes = [0u8; 16];
  std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
  Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl Leases {
  fn prune(&mut self, now: Instant) {
    self.clients.retain(|_, lease| lease.expires > now);
    self.legacy = self.legacy.filter(|until| *until > now);
  }

  pub(crate) fn register(
    &mut self,
    lease: Option<&str>,
    demand: ProcessDemand,
    now: Instant,
  ) -> Result<String, Error> {
    self.register_with(lease, demand, now, token)
  }

  fn register_with(
    &mut self,
    lease: Option<&str>,
    demand: ProcessDemand,
    now: Instant,
    mut token: impl FnMut() -> io::Result<String>,
  ) -> Result<String, Error> {
    self.prune(now);
    let id = if let Some(id) = lease {
      if !valid(id) {
        return Err(Error::Invalid);
      }
      if !self.clients.contains_key(id) {
        return Err(Error::Expired);
      }
      id.to_string()
    } else {
      if self.clients.len() >= LIMIT {
        return Err(Error::Capacity);
      }
      (0..4)
        .find_map(|_| token().ok().filter(|id| valid(id) && !self.clients.contains_key(id)))
        .ok_or(Error::Unavailable)?
    };
    self.clients.insert(id.clone(), Lease { demand, expires: now + Duration::from_millis(TTL_MS) });
    Ok(id)
  }

  /// A missing header is the old API. A present invalid/expired token never becomes legacy.
  pub(crate) fn observe(&mut self, header: Option<&str>, now: Instant) -> Result<(), Error> {
    self.prune(now);
    let until = now + Duration::from_millis(TTL_MS);
    if let Some(id) = header {
      if !valid(id) {
        return Err(Error::Invalid);
      }
      self.clients.get_mut(id).ok_or(Error::Expired)?.expires = until;
    } else {
      self.legacy = Some(until);
    }
    Ok(())
  }

  pub(crate) fn demand(&mut self, now: Instant) -> ProcessDemand {
    self.prune(now);
    self
      .clients
      .values()
      .map(|lease| lease.demand)
      .chain(self.legacy.map(|_| ProcessDemand::Details))
      .max()
      .unwrap_or(ProcessDemand::None)
  }

  pub(crate) fn deadline(&mut self, now: Instant) -> Option<Instant> {
    self.prune(now);
    self.clients.values().map(|lease| lease.expires).chain(self.legacy).min()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn register(leases: &mut Leases, number: u64, demand: ProcessDemand, at: Instant) -> String {
    leases.register_with(None, demand, at, || Ok(format!("{number:032x}"))).unwrap()
  }

  #[test]
  fn whole_snapshot_intervals_respect_slower_system_settings() {
    for system in [100, 1_000, 4_000, 10_000, u32::MAX] {
      assert_eq!(interval_ms(ProcessDemand::None, system), None);
      assert_eq!(interval_ms(ProcessDemand::Summary, system), Some(system.max(5_000)));
      assert_eq!(interval_ms(ProcessDemand::Details, system), Some(system.max(1_000)));
    }
  }

  #[test]
  fn independent_tabs_expire_and_neither_can_downgrade_another() {
    let now = Instant::now();
    let mut leases = Leases::default();
    assert_eq!(leases.demand(now), ProcessDemand::None);
    let summary = register(&mut leases, 1, ProcessDemand::Summary, now);
    let details = register(&mut leases, 2, ProcessDemand::Details, now);
    assert_eq!(leases.demand(now), ProcessDemand::Details);
    leases.register(Some(&summary), ProcessDemand::None, now).unwrap();
    assert_eq!(leases.demand(now), ProcessDemand::Details);
    let later = now + Duration::from_secs(10);
    leases.observe(Some(&summary), later).unwrap();
    assert_eq!(leases.deadline(later), Some(now + Duration::from_millis(TTL_MS)));
    assert_eq!(leases.demand(now + Duration::from_secs(15)), ProcessDemand::None);
    assert_eq!(leases.observe(Some(&details), later + Duration::from_secs(5)), Err(Error::Expired));
    assert_eq!(
      leases.clients.len(),
      1,
      "None is still a valid managed lease, without native demand"
    );
  }

  #[test]
  fn legacy_get_has_its_own_ttl_and_a_managed_header_never_impersonates_it() {
    let now = Instant::now();
    let mut leases = Leases::default();
    let id = register(&mut leases, 1, ProcessDemand::Summary, now);
    assert_eq!(leases.observe(Some("managed-v1"), now), Err(Error::Invalid));
    assert_eq!(leases.observe(Some(&format!("{:032x}", 99)), now), Err(Error::Expired));
    assert_eq!(leases.demand(now), ProcessDemand::Summary);
    leases.observe(None, now).unwrap();
    leases.register(Some(&id), ProcessDemand::None, now).unwrap();
    assert_eq!(leases.demand(now), ProcessDemand::Details);
    assert_eq!(leases.demand(now + Duration::from_secs(15)), ProcessDemand::None);
    assert_eq!(leases.observe(Some(&id), now + Duration::from_secs(15)), Err(Error::Expired));
    assert_eq!(leases.demand(now + Duration::from_secs(15)), ProcessDemand::None);
  }

  #[test]
  fn registry_is_bounded_and_failed_updates_leave_other_clients_unchanged() {
    let now = Instant::now();
    let mut leases = Leases::default();
    for id in 0..LIMIT {
      register(&mut leases, id as u64, ProcessDemand::Summary, now);
    }
    assert_eq!(leases.register(None, ProcessDemand::Details, now), Err(Error::Capacity));
    assert_eq!(leases.register(Some(""), ProcessDemand::None, now), Err(Error::Invalid));
    assert_eq!(
      leases.register(Some(&format!("{:032x}", 99)), ProcessDemand::None, now),
      Err(Error::Expired)
    );
    assert_eq!(leases.demand(now), ProcessDemand::Summary);
    assert!(
      leases
        .register_with(None, ProcessDemand::Details, now + Duration::from_secs(15), || Err(
          io::Error::other("entropy unavailable")
        ))
        .is_err()
    );
    assert_eq!(leases.demand(now + Duration::from_secs(15)), ProcessDemand::None);
    register(&mut leases, 100, ProcessDemand::Details, now + Duration::from_secs(15));
    assert_eq!(leases.clients.len(), 1);
  }
}
