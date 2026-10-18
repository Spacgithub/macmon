//! OS memory pressure and swap activity, separate from the stable memory-usage metrics.

use std::mem::{offset_of, size_of};
use std::ptr::null_mut;
use std::time::Instant;

use serde::Serialize;

unsafe extern "C" {
  fn host_page_size(host: libc::host_t, size: *mut libc::vm_size_t) -> libc::kern_return_t;
  fn mach_port_deallocate(task: libc::mach_port_t, name: libc::mach_port_t) -> libc::kern_return_t;
}

/// System memory pressure reported by macOS, independent of the fraction of RAM in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
#[serde(rename_all = "lowercase")]
pub enum MemoryPressure {
  /// The operating system reports normal memory pressure.
  Normal,
  /// The operating system reports elevated memory pressure.
  Warning,
  /// The operating system reports critical memory pressure.
  Critical,
}

/// Memory activity from the same successful acquisition as [`crate::Sampler::get_metrics`].
///
/// Swap rates are page-equivalent bytes per second from the kernel's cumulative `swapins` and
/// `swapouts` counters and actual host page size. They are neither changes in allocated swap
/// space nor the SSD's total physical I/O or wear. In a VM these describe the guest OS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
pub struct MemoryActivity {
  /// OS pressure level; `None` if the interface is unavailable or its value is unknown.
  pub pressure: Option<MemoryPressure>,
  /// Swap-in page-equivalent bytes per second; `None` until two valid samples are available.
  pub swap_in_bytes_per_sec: Option<f64>,
  /// Swap-out page-equivalent bytes per second; `None` after a failed read or counter reset.
  pub swap_out_bytes_per_sec: Option<f64>,
}

#[derive(Clone, Copy)]
struct SwapCounters {
  incoming: u64,
  outgoing: u64,
  page_size: u64,
  sampled: Instant,
}

#[derive(Default)]
pub(crate) struct MemoryActivitySampler {
  latest: MemoryActivity,
  previous: Option<SwapCounters>,
}

impl MemoryActivitySampler {
  #[cfg(test)]
  pub(crate) fn fixture(latest: MemoryActivity) -> Self {
    Self { latest, previous: None }
  }

  pub(crate) fn latest(&self) -> &MemoryActivity {
    &self.latest
  }

  pub(crate) fn sample(&mut self) {
    self.observe(read_pressure(), read_swap_counters());
  }

  fn observe(&mut self, pressure: Option<MemoryPressure>, current: Option<SwapCounters>) {
    self.latest = MemoryActivity { pressure, ..MemoryActivity::default() };
    let previous = std::mem::replace(&mut self.previous, current);
    let Some((previous, current)) = previous.zip(current) else { return };
    let Some(elapsed) = current.sampled.checked_duration_since(previous.sampled) else { return };
    if elapsed.is_zero() || current.page_size == 0 || current.page_size != previous.page_size {
      return;
    }
    let Some((incoming, outgoing)) = current
      .incoming
      .checked_sub(previous.incoming)
      .zip(current.outgoing.checked_sub(previous.outgoing))
    else {
      return;
    };
    let bytes_per_page_second = current.page_size as f64 / elapsed.as_secs_f64();
    self.latest.swap_in_bytes_per_sec = Some(incoming as f64 * bytes_per_page_second);
    self.latest.swap_out_bytes_per_sec = Some(outgoing as f64 * bytes_per_page_second);
  }
}

fn pressure_level(raw: u32) -> Option<MemoryPressure> {
  // The sysctl converts internal XNU levels to dispatch memory-pressure values, not 0/1/3.
  match raw {
    1 => Some(MemoryPressure::Normal),
    2 => Some(MemoryPressure::Warning),
    4 => Some(MemoryPressure::Critical),
    _ => None,
  }
}

fn read_pressure() -> Option<MemoryPressure> {
  let mut value = 0u32;
  let mut size = size_of::<u32>();
  let status = unsafe {
    libc::sysctlbyname(
      c"kern.memorystatus_vm_pressure_level".as_ptr(),
      (&mut value as *mut u32).cast(),
      &mut size,
      null_mut(),
      0,
    )
  };
  if status == 0 && size == size_of::<u32>() { pressure_level(value) } else { None }
}

struct HostSendRight(libc::mach_port_t);

#[allow(deprecated)]
impl Drop for HostSendRight {
  fn drop(&mut self) {
    unsafe {
      mach_port_deallocate(libc::mach_task_self(), self.0);
    }
  }
}

fn contains_swap_fields(count: libc::mach_msg_type_number_t) -> bool {
  count as usize * size_of::<libc::integer_t>()
    >= offset_of!(libc::vm_statistics64, swapouts) + size_of::<u64>()
}

#[allow(deprecated)]
fn read_swap_counters() -> Option<SwapCounters> {
  let host = unsafe { libc::mach_host_self() };
  if host == libc::MACH_PORT_NULL as libc::mach_port_t {
    return None;
  }
  let host = HostSendRight(host);
  let mut page_size = 0;
  let mut stats = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
  let mut count = libc::HOST_VM_INFO64_COUNT;
  let status = unsafe {
    libc::host_statistics64(
      host.0,
      libc::HOST_VM_INFO64,
      (&mut stats as *mut libc::vm_statistics64).cast(),
      &mut count,
    )
  };
  if status != libc::KERN_SUCCESS || !contains_swap_fields(count) {
    return None;
  }
  if unsafe { host_page_size(host.0, &mut page_size) } != libc::KERN_SUCCESS || page_size == 0 {
    return None;
  }
  Some(SwapCounters {
    incoming: stats.swapins,
    outgoing: stats.swapouts,
    page_size: page_size as u64,
    sampled: Instant::now(),
  })
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::*;

  fn counters(incoming: u64, outgoing: u64, page_size: u64, sampled: Instant) -> SwapCounters {
    SwapCounters { incoming, outgoing, page_size, sampled }
  }

  #[test]
  fn rates_use_independent_counters_actual_page_size_and_elapsed_time() {
    let mut sampler = MemoryActivitySampler::default();
    let time = Instant::now();
    sampler.observe(Some(MemoryPressure::Warning), Some(counters(100, 200, 16384, time)));
    assert_eq!(
      sampler.latest(),
      &MemoryActivity { pressure: Some(MemoryPressure::Warning), ..Default::default() }
    );
    sampler.observe(
      Some(MemoryPressure::Normal),
      Some(counters(103, 207, 16384, time + Duration::from_millis(1500))),
    );
    assert_eq!(sampler.latest().swap_in_bytes_per_sec, Some(32768.0));
    assert!((sampler.latest().swap_out_bytes_per_sec.unwrap() - 7.0 * 16384.0 / 1.5).abs() < 1e-6);
    sampler.observe(
      Some(MemoryPressure::Normal),
      Some(counters(103, 207, 16384, time + Duration::from_secs(2))),
    );
    assert_eq!(sampler.latest().swap_in_bytes_per_sec, Some(0.0));
    assert_eq!(sampler.latest().swap_out_bytes_per_sec, Some(0.0));
  }

  #[test]
  fn failed_reads_and_counter_rollbacks_reset_the_baseline_without_fabricating_zero() {
    let mut sampler = MemoryActivitySampler::default();
    let time = Instant::now();
    sampler.observe(None, Some(counters(100, 200, 4096, time)));
    sampler.observe(Some(MemoryPressure::Critical), None);
    assert_eq!(sampler.latest().pressure, Some(MemoryPressure::Critical));
    assert!(sampler.latest().swap_in_bytes_per_sec.is_none());
    sampler.observe(None, Some(counters(200, 300, 4096, time + Duration::from_secs(2))));
    assert!(sampler.latest().swap_out_bytes_per_sec.is_none());
    sampler.observe(None, Some(counters(10, 310, 4096, time + Duration::from_secs(3))));
    assert!(
      sampler.latest().swap_in_bytes_per_sec.is_none()
        && sampler.latest().swap_out_bytes_per_sec.is_none()
    );
    sampler.observe(None, Some(counters(11, 312, 4096, time + Duration::from_secs(4))));
    assert_eq!(sampler.latest().swap_in_bytes_per_sec, Some(4096.0));
    assert_eq!(sampler.latest().swap_out_bytes_per_sec, Some(8192.0));
  }

  #[test]
  fn changed_page_size_and_non_increasing_clocks_leave_rates_unavailable() {
    let mut sampler = MemoryActivitySampler::default();
    let time = Instant::now();
    for current in [
      counters(1, 1, 4096, time),
      counters(2, 2, 16384, time + Duration::from_secs(1)),
      counters(3, 3, 16384, time + Duration::from_secs(1)),
      counters(4, 4, 16384, time),
      counters(5, 5, 0, time + Duration::from_secs(2)),
    ] {
      sampler.observe(None, Some(current));
      assert!(
        sampler.latest().swap_in_bytes_per_sec.is_none()
          && sampler.latest().swap_out_bytes_per_sec.is_none()
      );
    }
  }

  #[test]
  fn pressure_values_and_vm_structure_length_are_validated() {
    for (value, expected) in
      [(1, MemoryPressure::Normal), (2, MemoryPressure::Warning), (4, MemoryPressure::Critical)]
    {
      assert_eq!(pressure_level(value), Some(expected));
      assert_eq!(
        serde_json::to_value(expected).unwrap(),
        match value {
          1 => "normal",
          2 => "warning",
          _ => "critical",
        }
      );
    }
    for value in [0, 3, 7, u32::MAX] {
      assert_eq!(pressure_level(value), None);
    }
    let count = ((offset_of!(libc::vm_statistics64, swapouts) + size_of::<u64>())
      / size_of::<libc::integer_t>()) as libc::mach_msg_type_number_t;
    assert!(!contains_swap_fields(count - 1));
    assert!(contains_swap_fields(count));
    assert!(contains_swap_fields(libc::HOST_VM_INFO64_COUNT));
    assert_eq!(
      serde_json::to_value(MemoryActivity::default()).unwrap(),
      serde_json::json!({"pressure":null,"swap_in_bytes_per_sec":null,"swap_out_bytes_per_sec":null})
    );
  }
}
