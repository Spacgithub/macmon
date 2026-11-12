//! Shared library helpers.

pub(crate) fn zero_div<T: core::ops::Div<Output = T> + Default + PartialEq>(a: T, b: T) -> T {
  let zero: T = Default::default();
  if b == zero { zero } else { a / b }
}

/// Sum one direction only when every observed device has a measured rate.
pub(crate) fn sum_rates(mut rates: impl Iterator<Item = Option<f64>>) -> Option<u64> {
  let valid = |rate: f64| rate.is_finite() && rate >= 0.0;
  let first = rates.next()?.filter(|&rate| valid(rate))?;
  let total = rates.try_fold(first, |sum, rate| Some(sum + rate.filter(|&rate| valid(rate))?))?;
  valid(total).then_some(total as u64)
}

pub(crate) fn is_pmp_ane_channel(group: &str, subgroup: &str, channel: &str, unit: &str) -> bool {
  group == "PMP"
    && subgroup == "Energy Counters"
    && channel.starts_with("ANE")
    && matches!(unit, "mJ" | "uJ" | "nJ")
}

pub(crate) fn is_pmp_mem_channel(group: &str, subgroup: &str, channel: &str, unit: &str) -> bool {
  group == "PMP"
    && subgroup == "Energy Counters"
    && (channel.starts_with("DRAM") || channel.starts_with("DCS"))
    && matches!(unit, "mJ" | "uJ" | "nJ")
}

pub(crate) fn is_dcs_energy_model_channel(channel: &str, unit: &str) -> bool {
  channel.starts_with("DCS") && matches!(unit, "mJ" | "uJ" | "nJ")
}

pub(crate) fn is_clpc_energy_channel(
  group: &str,
  subgroup: &str,
  channel: &str,
  unit: &str,
) -> bool {
  group == "CLPC"
    && subgroup == "Energy Counters"
    && matches!(
      channel,
      "CPU Energy" | "GPU Energy" | "ANE" | "DRAM" | "DCS" | "Package Energy" | "AVE" | "ISP"
    )
    && matches!(unit, "nJ" | "mJ")
}

pub(crate) fn ioreport_channels_filter(
  group: &str,
  subgroup: &str,
  channel: &str,
  unit: &str,
) -> bool {
  if is_pmp_ane_channel(group, subgroup, channel, unit)
    || is_pmp_mem_channel(group, subgroup, channel, unit)
    || is_clpc_energy_channel(group, subgroup, channel, unit)
  {
    return true;
  }

  if group == "Energy Model" {
    return channel == "GPU Energy"
      || channel.ends_with("CPU Energy")
      || channel.starts_with("ANE")
      || channel.starts_with("DRAM")
      || is_dcs_energy_model_channel(channel, unit)
      || channel.starts_with("GPU SRAM");
  }

  if group == "CPU Stats" {
    return subgroup == "CPU Core Performance States";
  }

  group == "GPU Stats" && subgroup == "GPU Performance States"
}

#[cfg(test)]
mod tests {
  use super::{ioreport_channels_filter, sum_rates};

  #[test]
  fn aggregate_rates_distinguish_missing_from_measured_zero() {
    assert_eq!(sum_rates([Some(0.0), Some(0.0)].into_iter()), Some(0));
    assert_eq!(sum_rates([Some(1.5), Some(2.5)].into_iter()), Some(4));
    assert_eq!(sum_rates([Some(100.0), None].into_iter()), None);
    assert_eq!(sum_rates([Some(f64::NAN)].into_iter()), None);
    assert_eq!(sum_rates([].into_iter()), None);
  }

  #[test]
  fn subscribes_only_to_known_clpc_energy_counters() {
    for channel in ["CPU Energy", "GPU Energy", "ANE", "DRAM", "DCS", "Package Energy", "ISP"] {
      assert!(ioreport_channels_filter("CLPC", "Energy Counters", channel, "nJ"));
      assert!(!ioreport_channels_filter("CLPC", "Energy Counters", channel, "events"));
    }
    assert!(ioreport_channels_filter("CLPC", "Energy Counters", "AVE", "mJ"));
    assert!(!ioreport_channels_filter("CLPC", "Energy Counters", "VDEC", "nJ"));
    assert!(!ioreport_channels_filter("CLPC", "Performance States", "ANE", "nJ"));
  }

  #[test]
  fn subscribes_to_ane_energy_without_bandwidth_or_state_counters() {
    assert!(ioreport_channels_filter("Energy Model", "", "ANE", "mJ"));
    assert!(ioreport_channels_filter("PMP", "Energy Counters", "ANE", "mJ"));
    assert!(!ioreport_channels_filter("PMP", "Bandwidth", "ANE RD", "events"));
    assert!(!ioreport_channels_filter("PMP", "Energy Counters", "ANE", "events"));
    assert!(!ioreport_channels_filter("H11ANE", "H11ANE Power State", "ANE Power", ""));
  }

  #[test]
  fn subscribes_to_pmp_dram_total_without_its_frequency_split() {
    for channel in ["DRAM", "DCS"] {
      assert!(ioreport_channels_filter("Energy Model", "", channel, "mJ"));
      assert!(ioreport_channels_filter("PMP", "Energy Counters", channel, "mJ"));
    }
    assert!(!ioreport_channels_filter("PMP", "Energy Counters", "DRAM", "events"));
    assert!(!ioreport_channels_filter("PMP", "Bandwidth", "DRAM", "mJ"));
    for channel in ["F0", "F1", "F2", "DRAM"] {
      assert!(!ioreport_channels_filter("PMP", "DRAM Energy", channel, "mJ"));
    }
  }

  #[test]
  fn ignores_non_energy_dcs_channels_in_the_energy_model() {
    for unit in ["mJ", "uJ", "nJ"] {
      assert!(ioreport_channels_filter("Energy Model", "", "DCS0 Energy", unit));
    }
    assert!(!ioreport_channels_filter("Energy Model", "", "DCS Events", "events"));
  }
}
