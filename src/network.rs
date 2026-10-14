//! Rootless interface counters and rates measured between consecutive snapshots.

use std::ffi::CStr;
use std::io;
use std::mem::size_of;
use std::ptr::null_mut;
use std::time::Instant;

use libc::{CTL_NET, IFDATA_GENERAL, IFMIB_IFALLDATA, NETLINK_GENERIC, PF_LINK};
use serde::Serialize;

/// Traffic on one up, non-loopback network interface, including VPNs and bridges.
#[derive(Debug, Default, Clone, Serialize)]
pub struct NetworkMetric {
  /// Kernel interface name, such as `en0` or `utun0`.
  pub name: String,
  /// Cumulative received bytes; resets when the interface restarts.
  pub rx_bytes: u64,
  /// Cumulative transmitted bytes; resets when the interface restarts.
  pub tx_bytes: u64,
  /// Received bytes per second, unavailable without a baseline or after a counter reset.
  pub rx_bytes_per_sec: Option<f64>,
  /// Transmitted bytes per second, unavailable without a baseline or after a counter reset.
  pub tx_bytes_per_sec: Option<f64>,
}

#[derive(Default)]
pub(crate) struct NetworkSampler {
  baseline: Option<(Instant, Vec<NetworkMetric>)>,
}

impl NetworkSampler {
  pub(crate) fn begin(&mut self) {
    if self.baseline.is_none() {
      let _ = self.sample();
    }
  }

  pub(crate) fn sample(&mut self) -> Option<Vec<NetworkMetric>> {
    self.record(read_counters().ok(), Instant::now())
  }

  fn record(
    &mut self,
    data: Option<Vec<NetworkMetric>>,
    now: Instant,
  ) -> Option<Vec<NetworkMetric>> {
    let Some(mut counters) = data else {
      self.baseline = None;
      return None;
    };
    counters.sort_by(|a, b| a.name.cmp(&b.name));
    let prev = self.baseline.replace((now, counters.clone()));
    let elapsed =
      prev.as_ref().map_or(0.0, |(at, _)| now.saturating_duration_since(*at).as_secs_f64());
    for metric in &mut counters {
      let old = prev.as_ref().and_then(|(_, items)| items.iter().find(|x| x.name == metric.name));
      let rate = |current: u64, previous: Option<u64>| {
        if elapsed > 0.0 { Some(current.checked_sub(previous?)? as f64 / elapsed) } else { None }
      };
      metric.rx_bytes_per_sec = rate(metric.rx_bytes, old.map(|x| x.rx_bytes));
      metric.tx_bytes_per_sec = rate(metric.tx_bytes, old.map(|x| x.tx_bytes));
    }
    Some(counters)
  }
}

fn read_counters() -> io::Result<Vec<NetworkMetric>> {
  // IFALLDATA preserves 64 bits; NET_RT_IFLIST2 truncates non-platform binaries' byte counts.
  let mut mib = [CTL_NET, PF_LINK, NETLINK_GENERIC, IFMIB_IFALLDATA, 0, IFDATA_GENERAL];
  let name = mib.as_mut_ptr();
  for _ in 0..3 {
    let mut size = 0;
    if unsafe { libc::sysctl(name, 6, null_mut(), &mut size, null_mut(), 0) } != 0 {
      return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u8; size];
    let output = buf.as_mut_ptr().cast();
    if unsafe { libc::sysctl(name, 6, output, &mut size, null_mut(), 0) } == 0 {
      return parse_counters(buf.get(..size).ok_or(io::ErrorKind::InvalidData)?);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(libc::ENOMEM) {
      return Err(error);
    }
  }
  Err(io::ErrorKind::WouldBlock.into())
}

fn parse_counters(bytes: &[u8]) -> io::Result<Vec<NetworkMetric>> {
  let (chunks, remainder) = bytes.as_chunks::<{ size_of::<libc::ifmibdata>() }>();
  if !remainder.is_empty() {
    return Err(io::ErrorKind::InvalidData.into());
  }
  let mut metrics = Vec::new();
  for chunk in chunks {
    // The buffer need not be aligned; the SDK's if_data64 is packed to four bytes.
    let raw = unsafe { chunk.as_ptr().cast::<libc::ifmibdata>().read_unaligned() };
    if raw.ifmd_flags & (libc::IFF_UP | libc::IFF_LOOPBACK) as u32 != libc::IFF_UP as u32 {
      continue;
    }
    let name = raw.ifmd_name.map(|c| c as u8);
    let name = CStr::from_bytes_until_nul(&name).map_err(|_| io::ErrorKind::InvalidData)?;
    if name.is_empty() {
      continue;
    }
    metrics.push(NetworkMetric {
      name: name.to_string_lossy().into_owned(),
      rx_bytes: raw.ifmd_data.ifi_ibytes,
      tx_bytes: raw.ifmd_data.ifi_obytes,
      ..Default::default()
    });
  }
  Ok(metrics)
}

#[cfg(test)]
mod tests {
  use std::mem::size_of;
  use std::time::{Duration, Instant};

  use super::{NetworkMetric, NetworkSampler, parse_counters};

  fn counter(name: &str, rx_bytes: u64, tx_bytes: u64) -> NetworkMetric {
    NetworkMetric { name: name.into(), rx_bytes, tx_bytes, ..Default::default() }
  }

  #[test]
  fn uses_measured_elapsed_time_and_keeps_interfaces_sorted() {
    let mut sampler = NetworkSampler::default();
    let at = Instant::now();
    let base = u64::from(u32::MAX) + 10;
    let first =
      sampler.record(Some(vec![counter("utun0", 10, 20), counter("en0", base, base)]), at).unwrap();
    assert_eq!(first.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["en0", "utun0"]);
    assert!(first.iter().all(|x| x.rx_bytes_per_sec.is_none() && x.tx_bytes_per_sec.is_none()));
    // Includes caller work between polls: 500 bytes over 2.5 seconds, not the requested sleep.
    let next = sampler
      .record(
        Some(vec![counter("en0", base + 500, base + 1000), counter("utun0", 10, 20)]),
        at + Duration::from_millis(2500),
      )
      .unwrap();
    assert_eq!((next[0].rx_bytes_per_sec, next[0].tx_bytes_per_sec), (Some(200.0), Some(400.0)));
    assert_eq!((next[1].rx_bytes_per_sec, next[1].tx_bytes_per_sec), (Some(0.0), Some(0.0)));
  }

  #[test]
  fn counter_resets_are_independent_and_interfaces_get_fresh_baselines() {
    let mut sampler = NetworkSampler::default();
    let at = Instant::now();
    let _ = sampler.record(Some(vec![counter("en0", 100, 100), counter("gone", 100, 100)]), at);
    let next = sampler
      .record(
        Some(vec![counter("en0", 10, 150), counter("new", 5000, 9000)]),
        at + Duration::from_secs(1),
      )
      .unwrap();
    assert_eq!(next.len(), 2);
    assert_eq!((next[0].rx_bytes_per_sec, next[0].tx_bytes_per_sec), (None, Some(50.0)));
    assert_eq!((next[1].rx_bytes_per_sec, next[1].tx_bytes_per_sec), (None, None));
    let next = sampler
      .record(
        Some(vec![counter("gone", 1000, 1000), counter("en0", 30, 10)]),
        at + Duration::from_secs(2),
      )
      .unwrap();
    assert_eq!((next[0].rx_bytes_per_sec, next[0].tx_bytes_per_sec), (Some(20.0), None));
    assert_eq!((next[1].rx_bytes_per_sec, next[1].tx_bytes_per_sec), (None, None));
  }

  #[test]
  fn unavailable_source_clears_baseline_and_recovers_without_a_spike() {
    let mut sampler = NetworkSampler::default();
    let at = Instant::now();
    let _ = sampler.record(Some(vec![counter("en0", 1, 1)]), at);
    assert!(sampler.record(None, at + Duration::from_secs(1)).is_none());
    assert!(sampler.baseline.is_none());
    let next = sampler
      .record(Some(vec![counter("en0", 10000, 10000)]), at + Duration::from_secs(2))
      .unwrap();
    assert_eq!((next[0].rx_bytes_per_sec, next[0].tx_bytes_per_sec), (None, None));
    let zero = sampler
      .record(Some(vec![counter("en0", 20000, 20000)]), at + Duration::from_secs(2))
      .unwrap();
    assert_eq!((zero[0].rx_bytes_per_sec, zero[0].tx_bytes_per_sec), (None, None));
    let next = sampler
      .record(Some(vec![counter("en0", 20100, 20200)]), at + Duration::from_secs(3))
      .unwrap();
    assert_eq!((next[0].rx_bytes_per_sec, next[0].tx_bytes_per_sec), (Some(100.0), Some(200.0)));
    assert!(sampler.record(Some(vec![]), at + Duration::from_secs(4)).unwrap().is_empty());
  }

  fn raw_bytes(name: &str, flags: i32, rx: u64, tx: u64) -> Vec<u8> {
    let mut raw = unsafe { std::mem::zeroed::<libc::ifmibdata>() };
    for (out, byte) in raw.ifmd_name.iter_mut().zip(name.bytes()) {
      *out = byte as _;
    }
    raw.ifmd_flags = flags as u32;
    raw.ifmd_data.ifi_ibytes = rx;
    raw.ifmd_data.ifi_obytes = tx;
    unsafe {
      std::slice::from_raw_parts(
        (&raw as *const libc::ifmibdata).cast(),
        size_of::<libc::ifmibdata>(),
      )
      .to_vec()
    }
  }

  #[test]
  fn parses_unaligned_64_bit_counters_and_filters_down_and_loopback_interfaces() {
    let large = 1u64 << 40;
    let mut bytes = vec![0];
    bytes.extend(raw_bytes("en0", libc::IFF_UP, large, large + 1));
    bytes.extend(raw_bytes("lo0", libc::IFF_UP | libc::IFF_LOOPBACK, 100, 100));
    bytes.extend(raw_bytes("down", 0, 100, 100));
    bytes.extend(raw_bytes("utun0", libc::IFF_UP, 20, 30));
    let metrics = parse_counters(&bytes[1..]).unwrap();
    assert_eq!(metrics.len(), 2);
    assert_eq!(
      (metrics[0].name.as_str(), metrics[0].rx_bytes, metrics[0].tx_bytes),
      ("en0", large, large + 1)
    );
    assert_eq!(metrics[1].name, "utun0");
    assert!(metrics.iter().all(|x| x.rx_bytes_per_sec.is_none()));
    assert!(parse_counters(&[]).unwrap().is_empty());
    assert!(parse_counters(&bytes[1..bytes.len() - 1]).is_err());
    assert!(parse_counters(&raw_bytes("abcdefghijklmnop", libc::IFF_UP, 0, 0)).is_err());
    assert!(parse_counters(&raw_bytes("", libc::IFF_UP, 0, 0)).unwrap().is_empty());
  }
}
