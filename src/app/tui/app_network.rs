//! The application view shares the existing Network panel; `n` returns to interfaces.

use std::cell::Cell;
use std::collections::HashSet;

use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::App;
use super::boxes::{Titles, draw_box, hint};
use super::network::bytes;
use super::theme::{self, dim, text};
use crate::app_network::{Group, Snapshot};

#[derive(Debug, Default)]
pub(super) struct View {
  pub enabled: bool,
  pub snapshot: Option<Snapshot>,
  selected: usize,
  sort: usize,
  open: HashSet<String>,
  scroll: Cell<Option<usize>>,
  rendered_offset: Cell<usize>,
}

impl View {
  fn groups(&self) -> Vec<&Group> {
    let mut groups: Vec<_> = self.snapshot.iter().flat_map(|snapshot| &snapshot.groups).collect();
    groups.sort_by(|a, b| {
      let value = |group: &Group| match self.sort {
        0 => group.rx_bytes_per_sec,
        1 => group.tx_bytes_per_sec,
        2 => Some(group.rx_session_bytes as f64),
        _ => Some(group.tx_session_bytes as f64),
      };
      value(b)
        .is_some()
        .cmp(&value(a).is_some())
        .then_with(|| value(b).unwrap_or(0.0).total_cmp(&value(a).unwrap_or(0.0)))
        .then(a.name.cmp(&b.name))
        .then(a.id.cmp(&b.id))
    });
    groups
  }
  pub(super) fn update(&mut self, snapshot: Option<Snapshot>) {
    if let Some(sample) = &snapshot {
      self.open.retain(|id| sample.groups.iter().any(|group| &group.id == id));
    }
    self.snapshot = snapshot;
  }
  pub(super) fn key(&mut self, code: KeyCode) -> bool {
    let count = self.snapshot.as_ref().map_or(0, |sample| sample.groups.len());
    self.selected = self.selected.min(count.saturating_sub(1));
    match code {
      KeyCode::Char('s') => {
        self.sort = (self.sort + 1) % 4;
        self.selected = 0;
      }
      KeyCode::Up => {
        self.selected = self.selected.saturating_sub(1);
        self.scroll.set(None);
      }
      KeyCode::Down => {
        self.selected = (self.selected + 1).min(count.saturating_sub(1));
        self.scroll.set(None);
      }
      KeyCode::PageDown => self
        .scroll
        .set(Some(self.scroll.get().unwrap_or(self.rendered_offset.get()).saturating_add(10))),
      KeyCode::PageUp => self
        .scroll
        .set(Some(self.scroll.get().unwrap_or(self.rendered_offset.get()).saturating_sub(10))),
      KeyCode::Home => {
        self.selected = 0;
        self.scroll.set(None);
      }
      KeyCode::End => {
        self.selected = count.saturating_sub(1);
        self.scroll.set(None);
      }
      KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => {
        let Some(id) = self.groups().get(self.selected).map(|group| group.id.clone()) else {
          return true;
        };
        if code == KeyCode::Left || code == KeyCode::Char(' ') && self.open.contains(&id) {
          self.open.remove(&id);
        } else {
          self.open.insert(id);
        }
      }
      _ => return false,
    }
    true
  }
}

fn rate(value: Option<f64>) -> String {
  value.map_or_else(|| "pending".into(), |value| format!("{}/s", bytes(value)))
}

fn traffic_lines(
  lines: &mut Vec<Line<'static>>,
  name: String,
  rx: Option<f64>,
  tx: Option<f64>,
  width: usize,
) {
  let rates = format!("↓ {} ↑ {}", rate(rx), rate(tx));
  let combined = Line::from(text(format!("{name} · {rates}")));
  if combined.width() <= width {
    lines.push(combined);
  } else {
    lines.push(Line::from(text(name)));
    if Line::from(rates.clone()).width() <= width {
      lines.push(Line::from(text(rates)));
    } else {
      lines.push(Line::from(text(format!("  ↓ {}", rate(rx)))));
      lines.push(Line::from(text(format!("  ↑ {}", rate(tx)))));
    }
  }
}

impl App {
  pub(super) fn render_app_network(&self, frame: &mut Frame, area: Rect, detailed: bool) {
    let inner = draw_box(
      frame,
      area,
      Titles::new(vec![theme::value("4 Network · Apps", theme::SECTION)])
        .right(hint("n", "interfaces")),
    );
    let Some(snapshot) = &self.app_network.snapshot else {
      frame.render_widget(
        Paragraph::new(dim("Application traffic unavailable from this collector")),
        inner,
      );
      return;
    };
    let age = snapshot.sampled.elapsed().as_secs();
    let status = if snapshot.state == "unavailable" {
      "Unavailable / retained"
    } else if age > 6 || !self.source_live() {
      "Stale / retained"
    } else if snapshot.state == "warming" {
      "Rates warming"
    } else {
      "Observed socket traffic"
    };
    let mut lines = vec![Line::from(dim(format!(
      "{status} · {age}s old · {} excluded",
      snapshot.excluded_processes
    )))];
    let labels = ["RX rate", "TX rate", "RX session", "TX session"];
    if detailed {
      lines.push(Line::from(dim(format!(
        "s sort: {} · Space/←→ members · ↑↓ select",
        labels[self.app_network.sort]
      ))));
    }
    let mut selection_line = 0;
    for (index, group) in self.app_network.groups().into_iter().enumerate() {
      if !detailed && index >= 3 {
        break;
      }
      let selected = index == self.app_network.selected;
      if selected {
        selection_line = lines.len();
      }
      let open = self.app_network.open.contains(&group.id);
      traffic_lines(
        &mut lines,
        format!(
          "{}{} {} ×{}",
          if detailed && selected { ">" } else { " " },
          if open && detailed { "−" } else { "+" },
          group.name,
          group.members.len()
        ),
        group.rx_bytes_per_sec,
        group.tx_bytes_per_sec,
        inner.width as usize,
      );
      if detailed {
        lines.push(Line::from(dim(format!(
          "   Observed session ↓ {} · ↑ {}",
          bytes(group.rx_session_bytes as f64),
          bytes(group.tx_session_bytes as f64)
        ))));
        if open {
          for member in &group.members {
            traffic_lines(
              &mut lines,
              format!("     {} {}", member.pid, member.name),
              member.rx_bytes_per_sec,
              member.tx_bytes_per_sec,
              inner.width as usize,
            );
            lines.push(Line::from(dim(format!(
              "       Session ↓ {} · ↑ {}",
              bytes(member.rx_session_bytes as f64),
              bytes(member.tx_session_bytes as f64)
            ))));
          }
        }
      }
    }
    if snapshot.groups.is_empty() {
      lines.push(Line::from(dim("No verified network processes reported")));
    }
    if detailed {
      lines.push(Line::from(dim(
        "TCP/UDP socket scope; session totals omit gaps and exited sockets; not interface totals",
      )));
    }
    let offset = if detailed {
      selection_line.saturating_sub(inner.height.saturating_sub(2) as usize)
    } else {
      0
    };
    let offset = self
      .app_network
      .scroll
      .get()
      .unwrap_or(offset)
      .min(lines.len().saturating_sub(inner.height as usize));
    self.app_network.rendered_offset.set(offset);
    if self.app_network.scroll.get().is_some() {
      self.app_network.scroll.set(Some(offset));
    }
    frame.render_widget(
      Paragraph::new(lines).scroll((offset.min(u16::MAX as usize) as u16, 0)),
      inner,
    );
  }
}

#[cfg(test)]
mod tests {
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

  use super::*;
  use crate::app_network::Member;
  use crate::config::Config;
  use crate::proc_groups::GroupKind;
  use crate::tui::Panel;

  fn sample() -> Snapshot {
    let group = |id: &str, rx| Group {
      id: id.into(),
      name: id.into(),
      kind: GroupKind::Application,
      members: vec![Member {
        pid: if id == "Browser" { 10 } else { 20 },
        instance: id.into(),
        name: format!("{id} helper"),
        rx_bytes_per_sec: rx,
        tx_bytes_per_sec: Some(20.0),
        rx_session_bytes: 4096,
        tx_session_bytes: 1024,
      }],
      rx_bytes_per_sec: rx,
      tx_bytes_per_sec: Some(20.0),
      rx_session_bytes: 4096,
      tx_session_bytes: 1024,
    };
    Snapshot {
      state: "ready".into(),
      sequence: 1,
      sampled: std::time::Instant::now(),
      groups: vec![group("Browser", Some(500.0)), group("Editor", None)],
      observed_processes: 2,
      excluded_processes: 1,
    }
  }
  fn draw(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render_app_network(frame, frame.area(), true)).unwrap();
    terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect::<String>()
  }

  #[test]
  fn network_views_sort_and_expand_members_independently_without_persisting_settings() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.panels.focus = Panel::Network;
    app.app_network.update(Some(sample()));
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    assert!(app.handle_panel_key(key(KeyCode::Char('n'))));
    assert!(app.app_network.enabled);
    let collapsed = draw(&app, 120, 24);
    assert!(collapsed.contains("Browser"));
    assert!(!collapsed.contains("Browser helper"));
    assert!(collapsed.contains("pending"));
    app.handle_panel_key(key(KeyCode::Right));
    assert!(draw(&app, 120, 24).contains("Browser helper"));
    app.handle_panel_key(key(KeyCode::Down));
    app.handle_panel_key(key(KeyCode::Right));
    let both = draw(&app, 120, 24);
    assert!(both.contains("Browser helper") && both.contains("Editor helper"));
    app.handle_panel_key(key(KeyCode::Left));
    assert!(app.app_network.open.contains("Browser"));
    assert!(!app.app_network.open.contains("Editor"));
    app.handle_panel_key(key(KeyCode::Char('s')));
    assert_eq!(app.app_network.sort, 1);
    app.handle_panel_key(key(KeyCode::Char('n')));
    assert!(!app.app_network.enabled);
    assert!(app.network.is_none()); // Application view did not replace interface telemetry.
  }

  #[test]
  fn paging_uses_the_visible_offset_without_overscrolling_or_jumping_above_selection() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut data = sample();
    let template = data.groups[0].members[0].clone();
    data.groups[0].members = (0..40)
      .map(|index| Member {
        pid: 1000 + index,
        name: format!("member-{index}"),
        instance: format!("instance-{index}"),
        ..template.clone()
      })
      .collect();
    app.app_network.update(Some(data));
    app.app_network.key(KeyCode::Right);
    for _ in 0..100 {
      app.app_network.key(KeyCode::PageDown);
    }
    let bottom = draw(&app, 100, 12);
    assert!(bottom.contains("member-39"));
    let offset = app.app_network.rendered_offset.get();
    assert_eq!(app.app_network.scroll.get(), Some(offset));
    app.app_network.key(KeyCode::PageUp);
    let up = draw(&app, 100, 12);
    assert_ne!(bottom, up, "the first PageUp must move after reaching the bottom");
    assert_eq!(app.app_network.rendered_offset.get(), offset.saturating_sub(10));

    app.app_network.key(KeyCode::End);
    let selected = draw(&app, 100, 12);
    assert!(selected.contains(">+ Editor"));
    let automatic = app.app_network.rendered_offset.get();
    assert!(automatic > 10);
    app.app_network.key(KeyCode::PageDown);
    let _ = draw(&app, 100, 12);
    assert!(app.app_network.rendered_offset.get() >= automatic);
  }

  #[test]
  fn retained_failure_ages_and_long_member_lists_remain_accessible_at_small_sizes() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut data = sample();
    data.state = "unavailable".into();
    data.sampled -= std::time::Duration::from_secs(15);
    let template = data.groups[0].members[0].clone();
    data.groups[0].members = (0..40)
      .map(|index| Member {
        pid: 1000 + index,
        name: format!("member-{index}"),
        instance: format!("instance-{index}"),
        ..template.clone()
      })
      .collect();
    app.app_network.update(Some(data));
    app.app_network.key(KeyCode::Right);
    assert!(draw(&app, 100, 12).contains("Unavailable / retained"));
    for _ in 0..12 {
      app.app_network.key(KeyCode::PageDown);
    }
    assert!(draw(&app, 100, 12).contains("member-39"));
    for (width, height) in [(28, 12), (80, 24), (160, 48)] {
      let _ = draw(&app, width, height);
    }
  }
}
