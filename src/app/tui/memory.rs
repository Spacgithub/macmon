//! OS memory pressure and logical paging, independent of RAM use and physical SSD I/O.

use macmon::{MemoryActivity, MemoryPressure};
use ratatui::text::{Line, Span};

use super::theme;

pub(super) fn pressure_name(value: Option<MemoryPressure>) -> &'static str {
  match value {
    Some(MemoryPressure::Normal) => "Normal",
    Some(MemoryPressure::Warning) => "Warning",
    Some(MemoryPressure::Critical) => "Critical",
    None => "Unavailable",
  }
}

pub(super) fn pressure_badge(value: Option<MemoryPressure>) -> Span<'static> {
  theme::value(format!("Pressure {}", pressure_name(value)), theme::memory_pressure(value))
}

pub(super) fn rate(value: Option<f64>) -> String {
  let Some(mut value) = value.filter(|value| value.is_finite() && *value >= 0.0) else {
    return "Unavailable".into();
  };
  let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let mut index = 0;
  while value >= 1024.0 && index < units.len() - 1 {
    value /= 1024.0;
    index += 1;
  }
  let number = if value == 0.0 {
    "0".into()
  } else if value >= 10_000.0 {
    format!("{value:.1e}")
  } else {
    format!("{value:.1}")
  };
  format!("{number} {}/s", units[index])
}

pub(super) fn activity_lines(activity: &MemoryActivity) -> Vec<Line<'static>> {
  vec![
    Line::from(vec![
      theme::dim("OS pressure "),
      theme::value(pressure_name(activity.pressure), theme::memory_pressure(activity.pressure)),
    ]),
    Line::from(theme::text(format!("Swap in  {}", rate(activity.swap_in_bytes_per_sec)))),
    Line::from(theme::text(format!("Swap out {}", rate(activity.swap_out_bytes_per_sec)))),
  ]
}

pub(super) fn rates_line(activity: &MemoryActivity) -> Line<'static> {
  Line::from(theme::dim(format!(
    "Swap I {} · O {}",
    rate(activity.swap_in_bytes_per_sec),
    rate(activity.swap_out_bytes_per_sec)
  )))
}

#[cfg(test)]
mod tests {
  use std::sync::RwLock;

  use macmon::{MemMetrics, Metrics, SocInfo};
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
  use ratatui::layout::Rect;
  use ratatui::style::Color;

  use super::*;
  use crate::config::Config;
  use crate::tui::{App, Event, Panel};

  fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|frame| app.render(frame)).unwrap();
    term.backend().buffer().content.iter().map(|cell| cell.symbol()).collect()
  }

  fn press(app: &mut App, code: KeyCode) {
    assert!(app.handle_key(KeyEvent::new(code, KeyModifiers::NONE)).is_continue());
  }

  fn update(app: &mut App, activity: MemoryActivity) {
    let metrics = Metrics {
      memory: MemMetrics {
        ram_usage: 8 << 30,
        ram_total: 32 << 30,
        swap_usage: 1 << 30,
        swap_total: 2 << 30,
      },
      ..Default::default()
    };
    assert!(
      app
        .handle_event(Event::Update(Box::new(metrics), Vec::new(), activity), &RwLock::new(1000))
        .is_continue()
    );
  }

  #[test]
  fn pressure_is_os_reported_and_rates_distinguish_unknown_zero_and_invalid_values() {
    assert_eq!(pressure_name(None), "Unavailable");
    for (pressure, label, color) in [
      (MemoryPressure::Normal, "Normal", Color::Green),
      (MemoryPressure::Warning, "Warning", Color::Yellow),
      (MemoryPressure::Critical, "Critical", Color::Red),
    ] {
      assert_eq!(pressure_name(Some(pressure)), label);
      assert_eq!(theme::memory_pressure(Some(pressure)), color);
    }
    for value in [None, Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
      assert_eq!(rate(value), "Unavailable");
    }
    assert_eq!(rate(Some(0.0)), "0 B/s");
    assert_eq!(rate(Some(1024.0)), "1.0 KiB/s");
    assert_eq!(rate(Some(2.5 * 1024.0 * 1024.0)), "2.5 MiB/s");
  }

  #[test]
  fn sample_updates_replace_memory_activity_while_folding_preserves_focus_and_process_sampling() {
    let mut app = App::from_parts(SocInfo::default(), Config::default());
    update(&mut app, MemoryActivity::default());
    let first = render(&mut app, 80, 24);
    assert!(first.contains("OS pressure Unavailable") && first.contains("Swap in  Unavailable"));
    let showing = app.procs_shown.showing();
    let activity = MemoryActivity {
      pressure: Some(MemoryPressure::Critical),
      swap_in_bytes_per_sec: Some(0.0),
      swap_out_bytes_per_sec: Some(2048.0),
    };
    update(&mut app, activity);
    let summary = render(&mut app, 80, 24);
    assert!(summary.contains("OS pressure Critical") && summary.contains("Swap in  0 B/s"));
    assert!(summary.contains("Swap out 2.0 KiB/s") && summary.contains("top 5"));
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('f'));
    let detail = render(&mut app, 80, 24);
    assert!(detail.contains("Swap 1.00 GiB / 2.00 GiB"));
    assert!(detail.contains("Logical paging, not SSD I/O"));
    assert_eq!(app.panels.focus, Panel::Memory);
    press(&mut app, KeyCode::Char('f'));
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.memory_activity, activity);
    assert_eq!(app.procs_shown.showing(), showing);
    update(&mut app, MemoryActivity::default());
    assert_eq!(app.memory_activity, MemoryActivity::default());
    assert!(render(&mut app, 80, 24).contains("OS pressure Unavailable"));
  }

  #[test]
  fn compact_units_fit_reasonable_narrow_windows_and_advanced_memory_keeps_its_activity() {
    let mut app = App::from_parts(SocInfo::default(), Config::default());
    update(
      &mut app,
      MemoryActivity {
        pressure: Some(MemoryPressure::Warning),
        swap_in_bytes_per_sec: Some(1024.0),
        swap_out_bytes_per_sec: Some(2048.0),
      },
    );
    app.panels.focus = Panel::Memory;
    app.panels.fullscreen = true;
    for (width, height) in [(0, 0), (1, 1), (5, 3), (28, 12), (60, 18), (80, 24)] {
      let view = render(&mut app, width, height);
      if width >= 28 {
        assert!(view.contains("Swap in  1.0 KiB/s") && view.contains("Swap out 2.0 KiB/s"));
      }
    }
    app.panels.fullscreen = false;
    app.cfg.panel_details = [true; 6];
    let advanced = render(&mut app, 200, 50);
    assert!(advanced.contains("Pressure Warning") && advanced.contains("Swap I 1.0 KiB/s"));
    assert!(advanced.contains("O 2.0 KiB/s"));
    let rates = rates_line(&app.memory_activity);
    let width = rates.width() as u16;
    // Two outer border cells precede the even GPU/RAM split; verify the actual box widths.
    for (padding, fits) in [(5, false), (6, true)] {
      let columns = (width + padding) * 2 + 2;
      let view = render(&mut app, columns, 24);
      let plan = app.layout(Rect::new(0, 0, columns, 24));
      let ram =
        plan.boxes.iter().find(|(metric, _)| *metric == crate::tui::layout::Metric::Ram).unwrap().1;
      assert_eq!(ram.width, width + padding);
      assert_eq!(view.contains("Swap I"), fits, "A whole rate line yields before cutting its unit");
      if fits {
        assert!(view.contains("Swap I 1.0 KiB/s · O 2.0 KiB/s"));
      }
    }
  }
}
