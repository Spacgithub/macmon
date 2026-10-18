//! Single-interface overview: VPN, bridge and physical counters are never added together.

use macmon::NetworkMetric;
use ratatui::prelude::*;

use super::App;
use super::theme::{self, dim, text};

pub(super) fn rate(value: Option<f64>) -> Option<f64> {
  value.filter(|value| value.is_finite() && *value >= 0.0)
}

fn traffic(metric: &NetworkMetric) -> f64 {
  // Halving only for ranking prevents two valid f64 rates overflowing their sum.
  rate(metric.rx_bytes_per_sec).unwrap_or(0.0) / 2.0
    + rate(metric.tx_bytes_per_sec).unwrap_or(0.0) / 2.0
}

fn compare(a: &NetworkMetric, b: &NetworkMetric) -> std::cmp::Ordering {
  let valid = |metric: &NetworkMetric| {
    rate(metric.rx_bytes_per_sec).is_some() || rate(metric.tx_bytes_per_sec).is_some()
  };
  valid(b).cmp(&valid(a)).then_with(|| traffic(b).total_cmp(&traffic(a))).then(a.name.cmp(&b.name))
}

pub(super) fn counts(metrics: &[NetworkMetric]) -> (usize, usize) {
  let active = metrics
    .iter()
    .filter(|metric| {
      rate(metric.rx_bytes_per_sec).is_some_and(|value| value > 0.0)
        || rate(metric.tx_bytes_per_sec).is_some_and(|value| value > 0.0)
    })
    .count();
  let pending = metrics
    .iter()
    .filter(|metric| {
      rate(metric.rx_bytes_per_sec).is_none() || rate(metric.tx_bytes_per_sec).is_none()
    })
    .count();
  (active, pending)
}

pub(super) fn sort(metrics: &mut [NetworkMetric]) {
  metrics.sort_by(compare);
}

pub(super) fn bytes(mut value: f64) -> String {
  let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let mut index = 0;
  while value >= 1024.0 && index < units.len() - 1 {
    value /= 1024.0;
    index += 1;
  }
  if value >= 10_000.0 {
    format!("{value:.0e} {}", units[index])
  } else {
    format!("{value:.1} {}", units[index])
  }
}

fn speed(value: Option<f64>) -> String {
  rate(value).map_or_else(|| "pending".into(), |value| format!("{}/s", bytes(value)))
}

fn pair(lines: &mut Vec<Line<'static>>, left: Line<'static>, right: Line<'static>, width: usize) {
  let combined = Line::from([left.spans.clone(), vec![dim(" · ")], right.spans.clone()].concat());
  if combined.width() <= width {
    lines.push(combined);
  } else {
    lines.extend([left, right]);
  }
}

impl App {
  pub(super) fn network_status(&self) -> &'static str {
    match self.network.as_deref() {
      None if !self.network_sampled => "Waiting for sample",
      None => "Collection unavailable",
      Some([]) => "No reported interfaces",
      Some(metrics) if counts(metrics).0 > 0 => "Observed traffic",
      Some(metrics) if counts(metrics).1 > 0 => "Rates pending",
      Some(_) => "Reported rates are zero",
    }
  }

  pub(super) fn network_overview(&self, width: usize) -> Vec<Line<'static>> {
    let Some(metrics) = &self.network else {
      return vec![Line::from(dim(if self.network_sampled {
        "Network collection unavailable"
      } else {
        "Waiting for network sample"
      }))];
    };
    if metrics.is_empty() {
      return vec![Line::from(dim("No reported interfaces"))];
    }
    let (active, pending) = counts(metrics);
    let counts = format!("{} observed · {active} traffic · {pending} pending", metrics.len());
    let counts = if counts.chars().count() <= width {
      counts
    } else {
      format!("N {} · A {active} · P {pending}", metrics.len())
    };
    let primary = metrics.iter().min_by(|a, b| compare(a, b)).unwrap();
    let mut lines = vec![
      Line::from(dim(counts)),
      Line::from(theme::value(format!("Auto · {}", primary.name), theme::SECTION)),
    ];
    let rates =
      format!("↓ {} · ↑ {}", speed(primary.rx_bytes_per_sec), speed(primary.tx_bytes_per_sec));
    if rates.chars().count() <= width {
      lines.push(Line::from(text(rates)));
    } else {
      lines.push(Line::from(theme::value(
        format!("↓ {}", speed(primary.rx_bytes_per_sec)),
        theme::DOWNLOAD,
      )));
      lines.push(Line::from(theme::value(
        format!("↑ {}", speed(primary.tx_bytes_per_sec)),
        theme::UPLOAD,
      )));
    }
    lines.push(Line::from(dim(self.network_status())));
    lines
  }

  pub(super) fn network_page_size(&self, area: Rect) -> usize {
    let width = usize::from(area.width.saturating_sub(2));
    if width < 66 {
      usize::from(area.height.saturating_sub(4))
    } else {
      usize::from(area.height.saturating_sub(3)).saturating_sub(self.network_overview(width).len())
    }
  }

  pub(super) fn network_detail_lines(&self, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, metric) in self.network.as_deref().unwrap_or_default().iter().enumerate() {
      lines.push(Line::from(theme::value(
        format!("{}{}", metric.name, if index == 0 { " · Auto" } else { "" }),
        theme::SECTION,
      )));
      pair(
        &mut lines,
        Line::from(theme::value(format!("↓ {}", speed(metric.rx_bytes_per_sec)), theme::DOWNLOAD)),
        Line::from(theme::value(format!("↑ {}", speed(metric.tx_bytes_per_sec)), theme::UPLOAD)),
        width,
      );
      pair(
        &mut lines,
        Line::from(dim(format!("RX total {}", bytes(metric.rx_bytes as f64)))),
        Line::from(dim(format!("TX total {}", bytes(metric.tx_bytes as f64)))),
        width,
      );
    }
    lines
  }

  pub(super) fn network_detail_count(&self, area: Rect) -> usize {
    if area.width.saturating_sub(2) < 66 {
      self.network_detail_lines(usize::from(area.width.saturating_sub(2))).len()
    } else {
      self.network.as_ref().map_or(0, Vec::len)
    }
  }
}

#[cfg(test)]
mod tests {
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::crossterm::event::KeyCode;

  use super::*;
  use crate::config::Config;
  use crate::tui::Panel;

  fn metric(name: &str, rx: Option<f64>, tx: Option<f64>) -> NetworkMetric {
    NetworkMetric {
      name: name.into(),
      rx_bytes_per_sec: rx,
      tx_bytes_per_sec: tx,
      ..Default::default()
    }
  }

  fn overview(app: &App, width: usize) -> String {
    app.network_overview(width).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
  }

  #[test]
  fn collection_states_and_global_pending_are_distinct_from_a_selected_zero_interface() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    assert!(overview(&app, 38).contains("Waiting for network sample"));
    app.network_sampled = true;
    assert!(overview(&app, 38).contains("Network collection unavailable"));
    app.network = Some(Vec::new());
    assert!(overview(&app, 38).contains("No reported interfaces"));
    app.network = Some(vec![metric("anpi0", None, None), metric("en0", Some(0.0), Some(0.0))]);
    let view = overview(&app, 38);
    assert!(
      view.contains("2 observed · 0 traffic · 1 pending") && view.contains("Auto · en0"),
      "{view}"
    );
    assert!(view.contains("↓ 0.0 B/s · ↑ 0.0 B/s") && view.contains("Rates pending"));
    assert!(!view.contains("zero") && !view.contains("idle"));
    app.network.as_mut().unwrap().remove(0);
    assert!(overview(&app, 38).contains("Reported rates are zero"));
    app.network = Some(vec![metric("utun0", Some(50.0), None)]);
    let view = overview(&app, 38);
    assert!(view.contains("1 observed · 1 traffic · 1 pending") && view.contains("↑ pending"));
    assert!(view.contains("Observed traffic"));
  }

  #[test]
  fn ranks_single_interfaces_without_cross_interface_totals_and_breaks_ties_by_name() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut metrics = vec![
      metric("utun0", Some(1024.0), Some(512.0)),
      metric("en0", Some(1024.0), Some(512.0)),
      metric("bridge0", Some(1024.0), Some(512.0)),
      metric("aa-unknown", None, None),
    ];
    metrics[0].rx_bytes = u64::MAX;
    sort(&mut metrics);
    assert_eq!(
      metrics.iter().map(|metric| metric.name.as_str()).collect::<Vec<_>>(),
      ["bridge0", "en0", "utun0", "aa-unknown"]
    );
    app.network = Some(metrics);
    let view = overview(&app, 38);
    assert!(view.contains("Auto · bridge0") && view.contains("↓ 1.0 KiB/s · ↑ 512.0 B/s"));
    assert!(!view.contains("3.0 KiB/s") && !view.contains("1.5 KiB/s"));
    app.network.as_mut().unwrap().retain(|metric| metric.name != "bridge0");
    assert!(overview(&app, 38).contains("Auto · en0"));
  }

  #[test]
  fn huge_finite_partial_and_invalid_rates_keep_safe_ranking_counts_and_complete_units() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut metrics = vec![
      metric("b", Some(f64::MAX), Some(f64::MAX)),
      metric("a", Some(f64::MAX), Some(f64::MAX)),
      metric("zero", Some(0.0), Some(0.0)),
      metric("unknown", Some(f64::NAN), Some(-1.0)),
      metric("partial", Some(f64::INFINITY), Some(f64::from_bits(1))),
    ];
    sort(&mut metrics);
    assert_eq!(metrics[0].name, "a");
    assert_eq!(metrics[1].name, "b");
    assert!(
      metrics.iter().position(|metric| metric.name == "zero").unwrap()
        < metrics.iter().position(|metric| metric.name == "unknown").unwrap()
    );
    app.network = Some(metrics);
    let view = overview(&app, 38);
    assert!(view.contains("5 observed · 3 traffic · 2 pending"), "{view}");
    for line in app.network_overview(26) {
      assert!(line.width() <= 26, "{line}");
    }
    assert!(view.contains("EiB/s") && !view.contains("NaN") && !view.contains("inf"));
  }

  #[test]
  fn narrow_expanded_view_adds_totals_immediately_and_reaches_all_interfaces_after_resizes() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut interfaces: Vec<_> =
      (0..40).map(|index| metric(&format!("utun{index:02}"), Some(0.0), Some(0.0))).collect();
    let mut primary = metric("en0", Some(1024.0 * 1024.0), Some(1024.0));
    primary.rx_bytes = 1 << 40;
    primary.tx_bytes = 1 << 30;
    interfaces.push(primary);
    app.update_metrics(macmon::Metrics { network: Some(interfaces), ..Default::default() });
    app.cfg.panel_details[Panel::Network as usize] = true;
    app.panels.focus = Panel::Network;
    let render = |app: &mut App, width, height| {
      let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
      terminal.draw(|frame| app.render(frame)).unwrap();
      terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect::<String>()
    };
    let mixed = render(&mut app, 80, 24);
    assert!(mixed.contains("RX total 1.0 TiB") && mixed.contains("TX total 1.0 GiB"), "{mixed}");
    app.panels.fullscreen = true;
    let narrow = render(&mut app, 28, 12);
    for expected in ["↓ 1.0 MiB/s", "↑ 1.0 KiB/s", "RX total 1.0 TiB", "TX total 1.0 GiB"] {
      assert!(narrow.contains(expected), "missing {expected}: {narrow}");
    }
    for line in app.network_detail_lines(26) {
      assert!(line.width() <= 26, "{line}");
    }
    for (width, height) in [(28, 12), (80, 24), (5, 3), (28, 12), (80, 24)] {
      render(&mut app, width, height);
      assert!(app.handle_key(KeyCode::End.into()).is_continue());
      let end = render(&mut app, width, height);
      if width >= 28 {
        assert!(end.contains("utun39"), "{width}x{height}: {end}");
      }
    }
    assert!(app.handle_key(KeyCode::Home.into()).is_continue());
    assert!(render(&mut app, 80, 24).contains("en0"));
  }
}
