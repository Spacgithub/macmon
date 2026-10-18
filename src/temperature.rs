//! Named temperature readings retained from the sampler's current sensor source.

use serde::Serialize;

/// Existing temperature group used to calculate the CPU and GPU averages.
///
/// SMC groups follow macmon's historical key-prefix rules; they do not identify
/// individual cores or establish a sensor's exact physical location.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
#[serde(rename_all = "lowercase")]
pub enum TemperatureGroup {
  /// A reading included in the CPU average.
  Cpu,
  /// A reading included in the GPU average.
  Gpu,
}

/// macOS interface that supplied a temperature reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
#[serde(rename_all = "lowercase")]
pub enum TemperatureSource {
  /// Apple System Management Controller.
  Smc,
  /// Apple IOHID temperature event.
  Hid,
}

/// One valid reading from the same acquisition as the reported temperature averages.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
pub struct TemperatureReading {
  /// Original SMC key or IOHID Product name, without an inferred physical location.
  pub name: String,
  /// Existing CPU/GPU averaging group.
  pub group: TemperatureGroup,
  /// Interface used for this sample; the sampler does not combine both sources.
  pub source: TemperatureSource,
  /// Temperature in Celsius, finite and in the range `0 < value <= 150`.
  pub celsius: f32,
}

#[cfg(test)]
mod tests {
  use super::{TemperatureGroup, TemperatureReading, TemperatureSource};

  #[test]
  fn named_readings_serialize_source_and_group_as_lowercase() {
    for (group, source, expected_group, expected_source) in [
      (TemperatureGroup::Cpu, TemperatureSource::Smc, "cpu", "smc"),
      (TemperatureGroup::Gpu, TemperatureSource::Hid, "gpu", "hid"),
    ] {
      let reading = TemperatureReading { name: "Tp01".into(), group, source, celsius: 42.5 };
      let json = serde_json::to_value(reading).unwrap();
      assert_eq!(
        json,
        serde_json::json!({
          "name": "Tp01", "group": expected_group, "source": expected_source, "celsius": 42.5
        })
      );
    }
  }
}
