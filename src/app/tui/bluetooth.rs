//! Bluetooth uses a separate ephemeral detail overlay; released panel preferences stay intact.

use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

use super::App;
use super::boxes::{Titles, draw_box, draw_scrollbar, hint};
use super::theme::{self, dim};
use crate::bluetooth::{Snapshot, State};

fn summary(snapshot: Option<&Snapshot>) -> String {
  let Some(snapshot) = snapshot else {
    return "Bluetooth waiting".into();
  };
  match snapshot.state {
    State::Warming => "Bluetooth waiting".into(),
    State::Unavailable => "Bluetooth unavailable".into(),
    State::Ready | State::Stale => {
      if snapshot.state == State::Ready && snapshot.controller == Some(false) {
        return "Bluetooth off".into();
      }
      let batteries = snapshot.devices.iter().filter(|device| !device.batteries.is_empty()).count();
      format!(
        "Bluetooth {}{} · battery {batteries}/{}",
        if snapshot.state == State::Stale { "cached " } else { "" },
        snapshot.devices.len(),
        snapshot.devices.len()
      )
    }
  }
}

impl App {
  pub(super) fn bluetooth_snapshot(&self) -> Option<Snapshot> {
    self.bluetooth.as_ref().map(|snapshot| {
      snapshot.aged(
        self
          .bluetooth_received
          .map_or(0, |received| received.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
      )
    })
  }

  pub(super) fn bluetooth_summary(&self) -> String {
    summary(self.bluetooth_snapshot().as_ref())
  }

  fn bluetooth_lines(&self) -> Vec<Line<'static>> {
    let snapshot = self.bluetooth_snapshot();
    let mut lines = vec![Line::from(theme::value(summary(snapshot.as_ref()), theme::SECTION))];
    let Some(snapshot) = snapshot else {
      return lines;
    };
    let controller = match snapshot.controller {
      Some(true) => "On",
      Some(false) => "Off",
      None => "Unavailable",
    };
    lines.push(Line::from(dim(format!(
      "Controller {controller} · report {}s old · refresh 30s",
      snapshot.age_ms.map_or_else(|| "?".into(), |age| (age / 1000).to_string())
    ))));
    if snapshot.state == State::Stale {
      lines
        .push(Line::from(dim("STALE / cached devices; connection and battery may have changed")));
    }
    lines.push(Line::from(dim("Connected-device report only; battery support varies by device.")));
    if snapshot.state == State::Unavailable {
      lines.push(Line::from(dim(
        "macOS did not supply a usable report; this does not mean zero devices.",
      )));
    }
    if snapshot.state == State::Ready && snapshot.devices.is_empty() {
      lines.push(Line::from(dim("No connected devices reported.")));
    }
    for device in &snapshot.devices {
      lines.push(Line::from(""));
      lines
        .push(Line::from(theme::value(format!("{} / {}", device.name, device.kind), theme::TEXT)));
      if device.batteries.is_empty() {
        lines.push(Line::from(dim("Battery unavailable / not reported by macOS")));
      }
      for battery in &device.batteries {
        lines.push(Line::from(format!("{} battery: {}%", battery.component, battery.percent)));
      }
    }
    lines
  }

  pub(super) fn render_bluetooth(&mut self, frame: &mut Frame, offset: usize) {
    self.bluetooth_rendered_age =
      self.bluetooth_snapshot().and_then(|snapshot| snapshot.age_ms).map(|age| age / 1000);
    self.placements.clear();
    self.fan_motion_visible = false;
    self.panels.targets.clear();
    self.proc_view.clear_targets();
    frame.render_widget(Clear, frame.area());
    let inner = draw_box(
      frame,
      frame.area(),
      Titles::new(theme::value("Bluetooth / connected accessories", theme::SECTION))
        .right(hint("b/Esc", "back")),
    );
    let mut rows = Vec::new();
    // Prewrap to count physical rows accurately at narrow sizes and keep every device reachable.
    for line in self.bluetooth_lines() {
      let text = line.to_string();
      let width = usize::from(inner.width.max(1));
      let mut row = String::new();
      let mut cells = 0;
      for grapheme in Line::from(text).styled_graphemes(Style::default()) {
        let character_width = Line::from(grapheme.symbol).width();
        if cells + character_width > width && !row.is_empty() {
          rows.push(Line::from(std::mem::take(&mut row)));
          cells = 0;
        }
        row.push_str(grapheme.symbol);
        cells += character_width;
      }
      rows.push(Line::from(row));
    }
    let offset = offset.min(rows.len().saturating_sub(usize::from(inner.height)));
    self.bluetooth_detail = Some(offset);
    frame.render_widget(
      Paragraph::new(rows.clone()).scroll((offset.min(u16::MAX as usize) as u16, 0)),
      inner,
    );
    draw_scrollbar(frame, frame.area(), inner, rows.len(), offset);
  }
}

#[cfg(test)]
mod tests {
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::crossterm::event::KeyCode;

  use super::*;
  use crate::config::Config;

  #[test]
  fn detail_overlay_keeps_legacy_preferences_and_all_battery_components_reachable() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.bluetooth = Some(Snapshot {
      state: State::Ready,
      controller: Some(true),
      age_ms: Some(1000),
      devices: (0..30)
        .map(|index| crate::bluetooth::Device {
          id: format!("bt-{index:016x}"),
          name: format!(
            "Accessory {index:02} 中文很长的设备名称🎧e\u{301}{}",
            "蓝牙🎵e\u{301}".repeat(12)
          ),
          kind: "Headphones".into(),
          batteries: ["left", "right", "case"]
            .into_iter()
            .map(|component| crate::bluetooth::Battery { component: component.into(), percent: 50 })
            .collect(),
        })
        .collect(),
      ..Default::default()
    });
    let cfg = serde_json::to_value(&app.cfg).unwrap();
    assert!(app.handle_key(KeyCode::Char('b').into()).is_continue());
    for (width, height) in [(80, 24), (28, 12), (8, 4), (80, 24)] {
      let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
      terminal.draw(|frame| app.render(frame)).unwrap();
      assert!(app.handle_key(KeyCode::End.into()).is_continue());
      terminal.draw(|frame| app.render(frame)).unwrap();
      let text: String =
        terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect();
      if width >= 28 {
        assert!(text.contains("case battery: 50%"), "{text}");
      }
    }
    for key in ['a', 'k', 'd', 'g', '+'] {
      assert!(app.handle_key(KeyCode::Char(key).into()).is_continue());
    }
    assert_eq!(serde_json::to_value(&app.cfg).unwrap(), cfg);
    assert!(!app.kill.is_open());
    assert!(app.handle_key(KeyCode::Esc.into()).is_continue());
    assert!(app.bluetooth_detail.is_none());
  }

  #[test]
  fn missing_empty_and_stale_reports_have_distinct_summaries() {
    assert_eq!(summary(None), "Bluetooth waiting");
    let mut snapshot = Snapshot {
      state: State::Ready,
      controller: Some(false),
      age_ms: Some(0),
      ..Default::default()
    };
    assert_eq!(summary(Some(&snapshot)), "Bluetooth off");
    snapshot.state = State::Unavailable;
    assert_eq!(summary(Some(&snapshot)), "Bluetooth unavailable");
    snapshot.state = State::Stale;
    assert!(summary(Some(&snapshot)).contains("cached"));
  }

  #[test]
  fn compact_network_keeps_bluetooth_summary_and_access_to_details_at_80_by_24() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.bluetooth = Some(Snapshot {
      state: State::Ready,
      controller: Some(true),
      age_ms: Some(1000),
      devices: vec![crate::bluetooth::Device {
        id: "bt-0123456789abcdef".into(),
        name: "Mouse".into(),
        kind: "Mouse".into(),
        batteries: vec![crate::bluetooth::Battery { component: "main".into(), percent: 100 }],
      }],
      ..Default::default()
    });
    app.update_metrics(macmon::Metrics {
      network: Some(vec![macmon::NetworkMetric {
        name: "en0".into(),
        rx_bytes_per_sec: Some(0.0),
        tx_bytes_per_sec: Some(0.0),
        ..Default::default()
      }]),
      ..Default::default()
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text: String =
      terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect();
    assert!(
      text.contains("BT 1") && text.contains("battery 1/1") && text.contains("b detail"),
      "{text}"
    );
  }
}
