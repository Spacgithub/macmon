//! Battery electrical flow is separate from the system/chip power estimates.

use ratatui::text::Line;

use super::{compute, theme};
use crate::battery::{Health, PowerSource, STALE_MS, Snapshot, State};

fn decimal(value: Option<f64>, unit: &str) -> String {
  value.filter(|v| v.is_finite()).map_or_else(|| "Unavailable".into(), |v| format!("{v:.2} {unit}"))
}
pub(super) fn brief(battery: &Snapshot, age: u64) -> String {
  if battery.state == State::Unavailable {
    "Battery N/A".into()
  } else if battery.age_ms.is_none_or(|cached| cached.saturating_add(age) > STALE_MS) {
    "Battery stale".into()
  } else {
    match battery.state {
      State::Absent => "No battery".into(),
      State::Unavailable => "Battery N/A".into(),
      State::Available => {
        battery.percent.map_or_else(|| "Battery —".into(), |value| format!("Battery {value:.0}%"))
      }
    }
  }
}
pub(super) fn lines(
  battery: Option<&Snapshot>,
  detailed: bool,
  age: u64,
  width: usize,
) -> Vec<Line<'static>> {
  let Some(battery) = battery else {
    return vec![Line::from(theme::dim("Battery telemetry unavailable"))];
  };
  let retained = battery.age_ms.is_none_or(|cached| cached.saturating_add(age) > STALE_MS);
  let state = match battery.state {
    State::Absent => "No internal battery".to_owned(),
    State::Unavailable => "Battery telemetry unavailable".to_owned(),
    State::Available => {
      let percent = battery.percent.map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
      let source = match battery.source {
        PowerSource::Ac => "AC",
        PowerSource::Battery => "Battery power",
        PowerSource::Ups => "UPS",
        PowerSource::Offline => "Offline",
        PowerSource::Unknown => "Source unknown",
      };
      let charge = match (battery.source, battery.charging) {
        (_, Some(true)) => "charging",
        (PowerSource::Battery, Some(false)) => "discharging",
        (_, Some(false)) => "not charging",
        (_, None) => "charge state unknown",
      };
      format!("Battery {percent} · {source} · {charge}")
    }
  };
  let mut lines =
    vec![Line::from(theme::text(format!("{}{state}", if retained { "STALE · " } else { "" })))];
  if !detailed {
    return lines;
  }
  let since = |value: Option<u64>| {
    value.map_or_else(
      || "unknown".into(),
      |v| format!("{:.1}s", v.saturating_add(age) as f64 / 1000.0),
    )
  };
  lines.push(Line::from(theme::dim(format!(
    "Battery acquired {} ago · registry {} ago",
    since(battery.age_ms),
    since(battery.registry_age_ms)
  ))));
  if battery.state != State::Available {
    return lines;
  }
  let health = match battery.health {
    Some(Health::Good) => "Good",
    Some(Health::Fair) => "Fair",
    Some(Health::Poor) => "Poor",
    None => "Unavailable",
  };
  let cycles = battery.cycles.map_or_else(|| "Unavailable".into(), |v| v.to_string());
  let fields = [
    format!("Health {health}"),
    format!("Cycles {cycles}"),
    format!("Nominal/design {}", decimal(battery.capacity_health_percent, "%")),
    format!("Battery temperature {}", decimal(battery.temperature_celsius, "°C")),
    format!("Voltage {}", decimal(battery.voltage_v, "V")),
    format!("Signed current {}", decimal(battery.current_a, "A")),
    format!("Battery electrical flow {}", decimal(battery.electrical_power_w, "W")),
    format!(
      "Estimated empty {}",
      battery.time_to_empty_min.map_or_else(|| "Unavailable".into(), |v| format!("{v} min"))
    ),
    format!(
      "Estimated full {}",
      battery.time_to_full_min.map_or_else(|| "Unavailable".into(), |v| format!("{v} min"))
    ),
  ];
  compute::fields(&mut lines, &fields, width);
  lines.push(Line::from(theme::dim("+ charging / − discharging · not system/chip power")));
  lines
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn summary_is_small_details_are_useful_and_unknown_is_not_zero() {
    let snapshot = Snapshot {
      state: State::Available,
      source: PowerSource::Ac,
      percent: Some(81.0),
      charging: Some(false),
      age_ms: Some(10),
      cycles: Some(150),
      health: Some(Health::Good),
      ..Default::default()
    };
    let summary = lines(Some(&snapshot), false, 0, 76);
    assert_eq!(summary.len(), 1);
    assert!(summary[0].to_string().contains("81% · AC · not charging"));
    let details = lines(Some(&snapshot), true, 0, 76);
    let text = details.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Cycles 150") && text.contains("Health Good"));
    assert!(text.contains("Battery temperature Unavailable") && !text.contains("0.00 W"));
    assert!(lines(Some(&snapshot), false, STALE_MS, 76)[0].to_string().starts_with("STALE"));
    assert_eq!(brief(&snapshot, 0), "Battery 81%");
    assert_eq!(brief(&snapshot, STALE_MS), "Battery stale");
    let absent = Snapshot { state: State::Absent, age_ms: Some(0), ..Default::default() };
    assert!(lines(Some(&absent), true, 0, 76)[0].to_string().contains("No internal battery"));
    assert_eq!(brief(&absent, 0), "No battery");
    assert_eq!(lines(None, true, 0, 76).len(), 1);
  }
}
