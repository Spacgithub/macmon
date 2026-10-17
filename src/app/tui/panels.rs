//! Independent summaries with a focused, scrollable view for details that outgrow a terminal.

use macmon::{TempMetrics, TemperatureGroup, TemperatureReading, TemperatureSource};
use ratatui::crossterm::event::{
  KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::boxes::{Titles, draw_box, draw_scrollbar, hint};
use super::cooling::Placement;
use super::theme::{self, dim, text};
use super::{App, cooling_layout, disk, help, memory};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(super) enum Panel {
  Compute,
  Memory,
  Power,
  Network,
  Cooling,
  #[default]
  Processes,
  Disk,
}

impl Panel {
  pub(super) const ALL: [Self; 7] = [
    Self::Compute,
    Self::Memory,
    Self::Power,
    Self::Network,
    Self::Cooling,
    Self::Processes,
    Self::Disk,
  ];
  pub(super) fn name(self) -> &'static str {
    match self {
      Self::Compute => "Compute",
      Self::Memory => "Memory",
      Self::Power => "Power",
      Self::Network => "Network",
      Self::Cooling => "Cooling",
      Self::Processes => "Processes",
      Self::Disk => "Disk",
    }
  }
}

#[derive(Debug, Default)]
pub(super) struct PanelState {
  pub(super) focus: Panel,
  pub(super) fullscreen: bool,
  offsets: [usize; 7],
  pages: [usize; 7],
  pub(super) targets: Vec<(Panel, Rect)>,
}

/// Latest raw sensors and 60 sampling windows; absent windows remain gaps, never zero heat.
#[derive(Debug, Default)]
pub(super) struct Temperatures {
  readings: Vec<TemperatureReading>,
  current: [Option<f32>; 2],
  history: [Vec<Option<f32>>; 2],
}

impl Temperatures {
  pub(super) fn push(&mut self, readings: Vec<TemperatureReading>, averages: &TempMetrics) {
    self.readings = readings;
    self.current = [averages.cpu_temp_avg, averages.gpu_temp_avg];
    for (history, value) in self.history.iter_mut().zip(self.current) {
      history.insert(0, value.filter(|value| value.is_finite()));
      history.truncate(60);
    }
  }

  fn lines(&self, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(dim("Last 60 samples · gaps mean unavailable · 0–150°C"))];
    for (index, group, label) in
      [(0, TemperatureGroup::Cpu, "CPU"), (1, TemperatureGroup::Gpu, "GPU")]
    {
      let readings: Vec<_> = self
        .readings
        .iter()
        .filter(|reading| reading.group == group && reading.celsius.is_finite())
        .collect();
      let min = readings.iter().map(|reading| reading.celsius).reduce(f32::min);
      let max = readings.iter().map(|reading| reading.celsius).reduce(f32::max);
      lines.push(Line::from(text(format!(
        "{label} sensors {} · min {} · max {}",
        readings.len(),
        degrees(min),
        degrees(max)
      ))));
      let history: Vec<_> = self.history[index].iter().flatten().copied().collect();
      let avg = (!history.is_empty()).then(|| history.iter().sum::<f32>() / history.len() as f32);
      let peak = history.iter().copied().reduce(f32::max);
      lines.push(Line::from(dim(format!(
        "Mean-temperature history · avg {} · peak {}",
        degrees(avg),
        degrees(peak)
      ))));
      let trend: String = self.history[index]
        .iter()
        .take(width)
        .rev()
        .map(|value| value.map_or('·', |value| bars((value as f64 / 150.0).clamp(0.0, 1.0))))
        .collect();
      lines.push(Line::from(theme::value(trend, if index == 0 { theme::CPU } else { theme::GPU })));
    }
    if let Some(hottest) = self
      .readings
      .iter()
      .filter(|reading| reading.celsius.is_finite())
      .max_by(|a, b| a.celsius.total_cmp(&b.celsius))
    {
      lines.push(Line::from(text(format!("Hotspot {} · {:.1}°C", hottest.name, hottest.celsius))));
    }
    lines.push(Line::from(dim("Raw sensor names identify readings, not physical cores")));
    for reading in &self.readings {
      let source = match reading.source {
        TemperatureSource::Smc => "SMC",
        TemperatureSource::Hid => "HID",
      };
      let group = match reading.group {
        TemperatureGroup::Cpu => "CPU",
        TemperatureGroup::Gpu => "GPU",
      };
      lines.push(Line::from(vec![
        theme::value(
          format!("{:.1}°C", reading.celsius),
          theme::gradient((reading.celsius as f64 - 30.0) / 70.0),
        ),
        dim(format!(" {group} {source} ")),
        text(reading.name.clone()),
      ]));
    }
    if self.readings.is_empty() {
      lines.push(Line::from(dim("Raw sensor details unavailable on this system")));
    }
    lines
  }
}

fn degrees(value: Option<f32>) -> String {
  value
    .filter(|value| value.is_finite())
    .map_or_else(|| "—".to_string(), |value| format!("{value:.1}°C"))
}

fn bars(ratio: f64) -> char {
  ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][(ratio.clamp(0.0, 1.0) * 7.0).round() as usize]
}

fn history(values: &[u64], max: u64, width: usize, color: Color) -> Line<'static> {
  let values: String = values
    .iter()
    .take(width.min(60))
    .rev()
    .map(|value| bars(*value as f64 / max.max(1) as f64))
    .collect();
  Line::from(theme::value(values, color))
}

/// Seven summaries fit at 80×24. Expansion borrows spare rows; focused view has full lists.
fn areas(
  area: Rect,
  details: [bool; 6],
  disk_details: bool,
  show_procs: bool,
  focus: Panel,
  fullscreen: bool,
) -> Vec<(Panel, Rect)> {
  if area.is_empty() {
    return Vec::new();
  }
  if fullscreen || area.height < 12 || area.width < 28 {
    return vec![(focus, area)];
  }
  let columns = if area.width >= 76 { 2 } else { 1 };
  let order =
    [Panel::Compute, Panel::Memory, Panel::Power, Panel::Cooling, Panel::Network, Panel::Processes];
  let panels: Vec<_> =
    order.into_iter().filter(|panel| show_procs || *panel != Panel::Processes).collect();
  let rows = panels.len().div_ceil(columns);
  if usize::from(area.height) < rows * 3 + 3 {
    return vec![(focus, area)];
  }
  let mut heights = vec![3usize; rows];
  let mut disk_height = 3usize;
  let desired: Vec<_> = panels
    .chunks(columns)
    .map(|row| {
      row
        .iter()
        .map(|panel| {
          if details[*panel as usize] {
            12
          } else {
            match panel {
              Panel::Network => 6,
              Panel::Memory => 6,
              Panel::Processes => 8,
              _ => 5,
            }
          }
        })
        .max()
        .unwrap_or(3)
    })
    .collect();
  // Prefer room for all five process-summary rows in the normal two-column overview.
  if columns == 2 && area.height >= 22 {
    // RAM has four live lines, Cooling three, and Network/Processes keep their Top 3/5.
    heights = vec![6, 5, if show_procs { 8 } else { 6 }];
  } else if columns == 2 && area.height >= 21 && show_procs {
    heights[rows - 1] = 8;
  }
  let mut spare =
    usize::from(area.height).saturating_sub(heights.iter().sum::<usize>() + disk_height);
  while spare > 0 {
    let mut used = false;
    for (height, wanted) in heights.iter_mut().zip(&desired) {
      if spare > 0 && *height < *wanted {
        *height += 1;
        spare -= 1;
        used = true;
      }
    }
    if spare > 0 && disk_details && disk_height < 12 {
      disk_height += 1;
      spare -= 1;
      used = true;
    }
    if !used {
      break;
    }
  }
  let mut result = Vec::new();
  let mut y = area.y;
  for (index, (row, height)) in panels.chunks(columns).zip(heights).enumerate() {
    let height = (height as u16).min(area.bottom().saturating_sub(y));
    let width = area.width / columns as u16;
    for (column, panel) in row.iter().enumerate() {
      let x = area.x + width * column as u16;
      let width = if column + 1 == columns { area.right() - x } else { width };
      let rect = Rect::new(x, y, width, height);
      if !rect.is_empty() {
        result.push((*panel, rect));
      }
    }
    y = y.saturating_add(height);
    if columns == 2 && index == 1 {
      result.push((Panel::Disk, Rect::new(area.x, y, area.width, disk_height as u16)));
      y += disk_height as u16;
    }
  }
  if columns == 1 {
    result.push((Panel::Disk, Rect::new(area.x, y, area.width, disk_height as u16)));
  }
  result
}

impl App {
  fn disk_age(&self) -> u64 {
    self
      .disk_sampled
      .map_or(0, |sampled| sampled.elapsed().as_millis().min(u64::MAX as u128) as u64)
  }

  /// The advanced renderer and image preflight must agree on the space above the Disk strip.
  pub(super) fn legacy_layout(
    &self,
    area: Rect,
  ) -> (super::LayoutPlan, Vec<Placement>, Option<Rect>) {
    let (mut original, original_fans) = self.cooling_layout(area, self.cfg.show_procs);
    if area.width < 76 || area.height < 24 {
      return (original, original_fans, None);
    }
    let metrics = Rect { height: area.height - 3, ..area };
    if let Some(procs) = original.proc {
      // Borrow process space, preserving the metric/fan geometry and six rows for a normal table.
      // At 80×24 its summary keeps the request/selection and a visible 6/f route to the whole list.
      let height =
        if self.cfg.disk_details { procs.height.saturating_sub(6).clamp(3, 12) } else { 3 };
      original.proc = (procs.height > height).then_some(Rect {
        y: procs.y + height,
        height: procs.height.saturating_sub(height),
        ..procs
      });
      let disk = Some(Rect { y: procs.y, height, ..procs });
      return (original, original_fans, disk);
    }
    let (plan, fans) = self.cooling_layout(metrics, self.cfg.show_procs);
    (plan, fans, Some(Rect { y: metrics.bottom(), height: 3, ..area }))
  }

  pub(super) fn thermal_label(&self) -> String {
    self
      .thermal_state
      .filter(|_| !self.soc.vm)
      .map_or_else(|| "Unavailable".into(), |state| format!("{state:?}"))
  }

  pub(super) fn cooling_detailed(&self) -> bool {
    self.cfg.panel_details[Panel::Cooling as usize]
      || (self.panels.fullscreen && self.panels.focus == Panel::Cooling)
  }

  pub(super) fn panels_legacy(&self) -> bool {
    !self.panels.fullscreen && self.cfg.panel_details.iter().all(|detail| *detail)
  }

  fn panel_areas(&self, area: Rect) -> Vec<(Panel, Rect)> {
    let body =
      if area.height >= 3 { Rect { y: area.y + 1, height: area.height - 2, ..area } } else { area };
    let focus = if !self.cfg.show_procs && self.panels.focus == Panel::Processes {
      Panel::Compute
    } else {
      self.panels.focus
    };
    areas(
      body,
      self.cfg.panel_details,
      self.cfg.disk_details,
      self.cfg.show_procs,
      focus,
      self.panels.fullscreen,
    )
  }

  pub(super) fn next_fan_placements(&self, area: Rect) -> Vec<Placement> {
    if !self.cooling_detailed() || self.help.is_some() || self.info.is_some() || self.kill.is_open()
    {
      return Vec::new();
    }
    if self.panels_legacy() {
      return self.legacy_layout(area).1;
    }
    self
      .panel_areas(area)
      .into_iter()
      .find(|(panel, _)| *panel == Panel::Cooling)
      .map(|(_, area)| {
        // The text ledger remains accessible while fan images use only the top eight rows.
        let inner = area.inner(Margin::new(1, 1));
        let card = Rect { height: inner.height.saturating_sub(3).min(8), ..inner };
        cooling_layout::panel_cards(
          card,
          self.cooling.items().len(),
          self.cell_pixels(),
          self.native_enabled(),
        )
      })
      .unwrap_or_default()
  }

  pub(super) fn handle_panel_key(&mut self, key: KeyEvent) -> bool {
    if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) {
      return false;
    }
    let index = self.panels.focus as usize;
    match key.code {
      KeyCode::Tab | KeyCode::BackTab => {
        let count = Panel::ALL.len();
        let step = if key.code == KeyCode::BackTab { count - 1 } else { 1 };
        let mut next = (index + step) % count;
        if !self.cfg.show_procs && next == Panel::Processes as usize {
          next = (next + step) % count;
        }
        self.panels.focus = Panel::ALL[next];
      }
      KeyCode::Char('1'..='7') => {
        let KeyCode::Char(digit) = key.code else { unreachable!() };
        let panel = Panel::ALL[(digit as u8 - b'1') as usize];
        if panel != Panel::Processes || self.cfg.show_procs {
          self.panels.focus = panel;
          self.cfg.toggle_panel(panel as usize);
        }
      }
      KeyCode::Enter => self.cfg.toggle_panel(index),
      KeyCode::Char('d') => self.cfg.toggle_all_panels(),
      KeyCode::Char('f') => self.panels.fullscreen = !self.panels.fullscreen,
      KeyCode::Esc if self.panels.fullscreen => self.panels.fullscreen = false,
      KeyCode::Char('/') if self.cfg.show_procs => {
        self.panels.focus = Panel::Processes;
        return false;
      }
      KeyCode::Char('p') => {
        if !self.cfg.show_procs {
          self.panels.focus = Panel::Processes;
        } else if self.panels.focus == Panel::Processes {
          self.panels.focus = Panel::Compute;
        }
        return false;
      }
      KeyCode::Up
      | KeyCode::Down
      | KeyCode::PageUp
      | KeyCode::PageDown
      | KeyCode::Home
      | KeyCode::End => {
        if self.panels.focus == Panel::Processes {
          if !self.panels_legacy() && !self.panels.fullscreen {
            self.panels.fullscreen = true;
          }
          return false;
        }
        if !self.cfg.panel_detailed(index) && !self.panels.fullscreen {
          return false;
        }
        let offset = &mut self.panels.offsets[index];
        let page = self.panels.pages[index].max(1);
        *offset = match key.code {
          KeyCode::Up => offset.saturating_sub(1),
          KeyCode::Down => offset.saturating_add(1),
          KeyCode::PageUp => offset.saturating_sub(page),
          KeyCode::PageDown => offset.saturating_add(page),
          KeyCode::Home => 0,
          KeyCode::End => usize::MAX,
          _ => *offset,
        };
      }
      _ => return false,
    }
    true
  }

  pub(super) fn handle_panel_mouse(&mut self, mouse: MouseEvent) -> bool {
    let point = Position::new(mouse.column, mouse.row);
    if self.panels_legacy()
      && !self
        .panels
        .targets
        .iter()
        .any(|(panel, area)| *panel == Panel::Disk && area.contains(point))
    {
      if self.procs_visible() && self.proc_view.contains_mouse(mouse) {
        self.panels.focus = Panel::Processes;
      }
      return false;
    }
    let Some(&(panel, area)) = self.panels.targets.iter().find(|(_, area)| area.contains(point))
    else {
      return true;
    };
    self.panels.focus = panel;
    self.proc_view.end_typing();
    match mouse.kind {
      MouseEventKind::Down(MouseButton::Left) if mouse.row == area.y => {
        self.cfg.toggle_panel(panel as usize)
      }
      MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if panel != Panel::Processes => {
        let code = if mouse.kind == MouseEventKind::ScrollUp { KeyCode::Up } else { KeyCode::Down };
        self.handle_panel_key(KeyEvent::new(code, KeyModifiers::NONE));
      }
      _ if panel == Panel::Processes => return false,
      _ => {}
    }
    true
  }

  fn panel_lines(&self, panel: Panel, detailed: bool, width: usize) -> Vec<Line<'static>> {
    match panel {
      Panel::Compute => {
        let mut lines = Vec::new();
        let mode = if self.soc.vm { crate::config::RatioMode::Active } else { self.cfg.ratio_mode };
        let frequency =
          |mhz| if mhz > 0 { format!(" · {mhz} MHz") } else { " · frequency unavailable".into() };
        for cluster in &self.clusters.items {
          let freq = &cluster.freq;
          let ratio = freq.ratio(mode);
          let label =
            if cluster.label == "_" { "CPU".into() } else { format!("{} CPU", cluster.label) };
          lines.push(Line::from(vec![
            theme::value(format!("{label} "), theme::CPU),
            text(format!("{:.1}%{}", ratio.ratio * 100.0, frequency(freq.freq_mhz))),
          ]));
          if detailed {
            lines.push(history(&ratio.items, 100, width, theme::CPU));
          }
        }
        if self.soc.vm {
          lines.push(Line::from(dim("GPU unavailable in a VM")));
          return lines;
        }
        let gpu = self.igpu_freq.ratio(self.cfg.ratio_mode);
        lines.push(Line::from(vec![
          theme::value("GPU ", theme::GPU),
          text(format!("{:.1}%{}", gpu.ratio * 100.0, frequency(self.igpu_freq.freq_mhz))),
        ]));
        if detailed {
          lines.push(history(&gpu.items, 100, width, theme::GPU));
        }
        lines
      }
      Panel::Memory => {
        let gib = |value| format!("{:.2} GiB", value as f64 / (1u64 << 30) as f64);
        let mut lines = vec![Line::from(text(format!(
          "RAM {} / {}",
          gib(self.mem.ram_usage),
          gib(self.mem.ram_total)
        )))];
        lines.extend(memory::activity_lines(&self.memory_activity));
        if detailed {
          lines.push(Line::from(dim(format!(
            "Swap {} / {}",
            gib(self.mem.swap_usage),
            gib(self.mem.swap_total)
          ))));
          lines.push(Line::from(dim("Logical paging, not SSD I/O")));
          lines.push(Line::from(dim("RAM history · latest 60 samples")));
          lines.push(history(&self.mem.items, self.mem.ram_total, width, theme::MEMORY));
        }
        lines
      }
      Panel::Power => {
        if self.soc.vm {
          return vec![Line::from(dim("Power measurements unavailable in a VM"))];
        }
        let (label, total) = if self.sys_power.items.first().is_some_and(|value| *value > 0) {
          ("System", &self.sys_power)
        } else {
          ("CPU+GPU+ANE", &self.all_power)
        };
        let mut lines =
          vec![Line::from(theme::value(format!("{label} {:.2} W", total.top_value), theme::POWER))];
        if detailed {
          lines.push(Line::from(dim("Source       now W    avg W   peak W")));
          for (name, store) in [
            ("CPU", &self.cpu_power),
            ("GPU", &self.gpu_power),
            ("ANE", &self.ane_power),
            (label, total),
          ] {
            lines.push(Line::from(text(format!(
              "{name:<12} {:>6.2}   {:>6.2}   {:>6.2}",
              store.top_value, store.avg_value, store.max_value
            ))));
          }
          lines.push(Line::from(dim("Power history · latest 60 samples")));
          lines.push(history(
            &total.items,
            total.items.iter().take(60).max().copied().unwrap_or(1),
            width,
            theme::POWER,
          ));
        } else {
          lines.push(Line::from(dim(format!(
            "CPU {:.2} · GPU {:.2} · ANE {:.2} W",
            self.cpu_power.top_value, self.gpu_power.top_value, self.ane_power.top_value
          ))));
        }
        lines
      }
      Panel::Cooling => {
        let current = if self.temperatures.current.iter().all(Option::is_none) {
          "CPU/GPU temperature unavailable".into()
        } else {
          format!(
            "CPU {} · GPU {}",
            degrees(self.temperatures.current[0]),
            degrees(self.temperatures.current[1])
          )
        };
        let mut lines = vec![
          Line::from(text(current)),
          Line::from(dim(if self.fans.label().is_empty() {
            "Fan readings unavailable".to_owned()
          } else {
            self.fans.label()
          })),
          Line::from(text(format!("OS thermal {}", self.thermal_label()))),
        ];
        if detailed {
          lines.push(Line::from(dim("OS state, not a Celsius threshold or proof of throttling")));
          lines.push(Line::from(dim("Nominal may also mean an unknown or unsupported platform")));
          lines.extend(self.temperatures.lines(width));
        }
        lines
      }
      Panel::Disk => disk::lines(self.disks.as_deref(), detailed, width, self.disk_age()),
      _ => Vec::new(),
    }
  }

  pub(super) fn render_panels(&mut self, frame: &mut Frame) {
    let area = frame.area();
    self.procs_fit = !area.is_empty();
    self.set_procs_visible(self.cfg.show_procs);
    self.panels.targets = self.panel_areas(area);
    self.placements = self.next_fan_placements(area);
    self.fan_motion_visible = false;
    if area.height >= 3 {
      let title = Line::from(vec![
        theme::value(" MACMON ", theme::SECTION),
        dim(self.soc.chip_name.clone()),
        text(format!(" · [{}] {}", self.panels.focus as usize + 1, self.panels.focus.name())),
      ]);
      frame.render_widget(title, Rect { height: 1, ..area });
      let footer = if self.proc_view.typing() {
        " Enter keep filter · Esc clear filter"
      } else if self.panels.fullscreen {
        " f / Esc back · ↑↓ PgUp/PgDn Home/End scroll · Tab switch · ? help"
      } else {
        " 1–7 toggle · Tab select · Enter detail · d all · f full · p procs · ? help"
      };
      frame
        .render_widget(Line::from(dim(footer)), Rect { y: area.bottom() - 1, height: 1, ..area });
    }
    for (panel, rect) in self.panels.targets.clone() {
      let detailed = self.cfg.panel_detailed(panel as usize) || self.panels.fullscreen;
      if panel == Panel::Disk {
        self.render_disk_panel(frame, rect);
        continue;
      }
      if panel == Panel::Processes {
        if detailed {
          self.render_proc_box(frame, rect);
        } else {
          self.render_proc_summary(frame, rect);
        }
        continue;
      }
      if panel == Panel::Network {
        let total = self.network.as_ref().map_or(0, Vec::len);
        let page = usize::from(rect.height.saturating_sub(3));
        let offset = if detailed {
          self.panels.offsets[panel as usize].min(total.saturating_sub(page))
        } else {
          0
        };
        self.panels.offsets[panel as usize] = offset;
        self.panels.pages[panel as usize] = page;
        self.render_network_page(frame, rect, offset, if detailed { usize::MAX } else { 3 });
        if detailed {
          draw_scrollbar(frame, rect, rect.inner(Margin::new(1, 1)), total, offset);
        }
        continue;
      }
      let focus = if self.panels.focus == panel { "●" } else { "○" };
      let state = if detailed { "detail" } else { "summary" };
      let titles = Titles::new(theme::value(
        format!("{focus} {} {} · {state}", panel as usize + 1, panel.name()),
        theme::SECTION,
      ))
      .right(hint("f", "full"));
      let mut inner = draw_box(frame, rect, titles);
      if panel == Panel::Cooling && !self.placements.is_empty() {
        self.cooling.render(frame, &self.placements);
        let bottom =
          self.placements.iter().map(|place| place.card.bottom()).max().unwrap_or(inner.y);
        inner.height = inner.bottom().saturating_sub(bottom);
        inner.y = bottom;
      }
      let lines = self.panel_lines(panel, detailed, inner.width as usize);
      let index = panel as usize;
      let shown = inner.height as usize;
      self.panels.pages[index] = shown;
      let offset = if detailed {
        self.panels.offsets[index].min(lines.len().saturating_sub(shown))
      } else {
        0
      };
      self.panels.offsets[index] = offset;
      let count = shown.min(lines.len().saturating_sub(offset));
      frame.render_widget(
        Paragraph::new(lines.clone()).scroll((offset.min(u16::MAX as usize) as u16, 0)),
        inner,
      );
      if detailed {
        draw_scrollbar(frame, rect, inner, lines.len(), offset);
      }
      if lines.len() > shown && rect.width >= 24 && rect.height >= 2 {
        let label = Line::from(dim(format!(" {count}/{} · f full ", lines.len())));
        frame.render_widget(
          label.right_aligned(),
          Rect {
            y: rect.bottom() - 1,
            height: 1,
            x: rect.x + 1,
            width: rect.width.saturating_sub(2),
          },
        );
      }
    }
    let overlay = self
      .panels
      .targets
      .iter()
      .find(|(panel, _)| *panel == Panel::Processes)
      .map_or(area, |(_, rect)| *rect);
    self.info.iter_mut().for_each(|info| info.render(frame, overlay));
    self.kill.render(frame, overlay);
    if let Some(scroll) = self.help {
      self.help = Some(help::render(frame, area, scroll));
    }
    if !self.native_enabled() && self.fans_visible() {
      self.cooling.rasterize(self.placements[0].side, theme::fan_mask_palette(), false);
      self.fan_base.clone_from(frame.buffer_mut());
      self.paint_text_fans(frame.buffer_mut());
    }
  }

  pub(super) fn render_disk_panel(&mut self, frame: &mut Frame, rect: Rect) {
    let index = Panel::Disk as usize;
    let detailed = self.cfg.disk_details || self.panels.fullscreen;
    let focus = if self.panels.focus == Panel::Disk { "●" } else { "○" };
    let total = self.disks.as_ref().map_or(0, Vec::len);
    let state = if detailed { "detail" } else { "summary" };
    let titles = Titles::new(theme::value(
      format!("{focus} 7 Disk · {total} devices · {state}"),
      theme::SECTION,
    ))
    .right(hint("f", if self.panels.fullscreen { "back" } else { "all disks" }));
    let inner = draw_box(frame, rect, titles);
    let lines = disk::lines(self.disks.as_deref(), detailed, inner.width as usize, self.disk_age());
    let shown = inner.height as usize;
    let offset =
      if detailed { self.panels.offsets[index].min(lines.len().saturating_sub(shown)) } else { 0 };
    self.panels.offsets[index] = offset;
    self.panels.pages[index] = shown;
    frame.render_widget(
      Paragraph::new(lines.clone()).scroll((offset.min(u16::MAX as usize) as u16, 0)),
      inner,
    );
    if detailed {
      draw_scrollbar(frame, rect, inner, lines.len(), offset);
    }
  }
}

#[cfg(test)]
mod tests {
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;

  use super::*;
  use crate::config::Config;
  use crate::procs::ProcInfo;

  fn app() -> App {
    App::from_parts(Default::default(), Config::default())
  }

  fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|frame| app.render(frame)).unwrap();
    term.backend().buffer().content.iter().map(|cell| cell.symbol()).collect()
  }

  fn press(app: &mut App, code: KeyCode) {
    assert!(app.handle_key(KeyEvent::new(code, KeyModifiers::NONE)).is_continue());
  }

  fn sensor(name: &str, group: TemperatureGroup, celsius: f32) -> TemperatureReading {
    TemperatureReading { name: name.into(), group, source: TemperatureSource::Smc, celsius }
  }

  #[test]
  fn thermal_state_is_sample_scoped_and_folding_keeps_the_cooling_summary_and_processes() {
    use std::sync::RwLock;

    use macmon::{Metrics, ThermalState};

    use crate::tui::Event;

    let mut app = app();
    let interval = RwLock::new(1000);
    render(&mut app, 80, 24);
    let showing = app.procs_shown.showing();
    for (state, label) in [
      (Some(ThermalState::Nominal), "Nominal"),
      (Some(ThermalState::Fair), "Fair"),
      (Some(ThermalState::Serious), "Serious"),
      (Some(ThermalState::Critical), "Critical"),
      (None, "Unavailable"),
    ] {
      let metrics = Metrics {
        temp: TempMetrics { cpu_temp_avg: Some(80.0), gpu_temp_avg: Some(80.0) },
        ..Default::default()
      };
      assert!(
        app
          .handle_event(
            Event::Update(
              Box::new(metrics),
              Vec::new(),
              Default::default(),
              state,
              None,
              std::time::Instant::now()
            ),
            &interval,
          )
          .is_continue()
      );
      assert_eq!(app.thermal_state, state);
      let summary = render(&mut app, 80, 24);
      assert!(summary.contains(&format!("OS thermal {label}")), "{summary}");
      assert!(summary.contains("top 5") && summary.contains("Network"));
      assert_eq!(app.cfg.panel_details, [false; 6]);
      app.panels.focus = Panel::Cooling;
      press(&mut app, KeyCode::Char('f'));
      for (width, height) in [(28, 12), (80, 24)] {
        assert!(render(&mut app, width, height).contains(&format!("OS thermal {label}")));
      }
      let detail = render(&mut app, 80, 24);
      assert!(detail.contains("not a Celsius threshold or proof of throttling"));
      assert!(detail.contains("unknown or unsupported platform"));
      press(&mut app, KeyCode::Char('f'));
      app.cfg.panel_details = [true; 6];
      let advanced = render(&mut app, 120, 40);
      assert!(advanced.contains(&format!("OS thermal {label}")), "{advanced}");
      app.cfg.panel_details = [false; 6];
      assert_eq!(app.procs_shown.showing(), showing);
    }
    app.soc.vm = true;
    app.thermal_state = Some(ThermalState::Critical);
    assert!(render(&mut app, 80, 24).contains("OS thermal Unavailable"));
    assert!(!render(&mut app, 80, 24).contains("OS thermal Critical"));
  }

  #[test]
  fn all_combinations_and_tiny_resizes_keep_rectangles_inside_and_disjoint() {
    for mask in 0..128 {
      let details = std::array::from_fn(|i| mask & (1 << i) != 0);
      let disk_details = mask & (1 << 6) != 0;
      for (width, height) in
        [(0, 0), (1, 1), (5, 3), (28, 12), (60, 18), (80, 24), (120, 40), (200, 50)]
      {
        for show in [false, true] {
          let area = Rect::new(3, 2, width, height);
          let plan = areas(area, details, disk_details, show, Panel::Cooling, false);
          for (i, (_, rect)) in plan.iter().enumerate() {
            assert_eq!(rect.intersection(area), *rect, "{mask} {area:?} {rect:?}");
            for (_, other) in &plan[..i] {
              assert!(rect.intersection(*other).is_empty());
            }
          }
          let mut app = app();
          app.cfg.panel_details = details;
          app.cfg.disk_details = disk_details;
          app.cfg.show_procs = show;
          render(&mut app, width, height);
        }
      }
    }
  }

  #[test]
  fn independent_and_global_controls_preserve_process_selection_sort_filter_and_sampling() {
    let mut app = app();
    render(&mut app, 80, 24);
    let showing = app.procs_shown.showing().unwrap();
    let procs = (0..12)
      .map(|index| ProcInfo {
        pid: 1000 + index,
        name: format!("worker{index}"),
        cpu_pct: index as f32,
        path: String::new(),
        user: "tester".into(),
        cpu_time_ns: 0,
        mem_bytes: 0,
        power_w: None,
        gpu_pct: 0.0,
        started: None,
      })
      .collect();
    app.update_procs(showing, procs);
    let summary = render(&mut app, 80, 24);
    assert!(
      summary.contains("worker11") && summary.contains("worker7") && !summary.contains("worker6")
    );
    assert!(summary.contains("top 5 / 12"));
    press(&mut app, KeyCode::Char('/'));
    for key in "worker167df".chars() {
      press(&mut app, KeyCode::Char(key));
    }
    assert_eq!(app.proc_view.filter(), "worker167df");
    let typing = render(&mut app, 80, 24);
    assert!(typing.contains("worker167df") && typing.contains("Enter keep filter"));
    assert_eq!(app.cfg.panel_details, [false; 6]);
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Down);
    let selected = app.proc_view.selected_pid();
    assert!(selected.is_some());
    press(&mut app, KeyCode::Char('f'));
    let sort = (app.proc_view.sort, app.proc_view.sort_desc);
    for digit in '1'..='7' {
      press(&mut app, KeyCode::Char(digit));
      render(&mut app, 80, 24);
      assert_eq!(app.procs_shown.showing(), Some(showing));
      assert_eq!(app.proc_view.selected_pid(), selected);
      assert_eq!((app.proc_view.sort, app.proc_view.sort_desc), sort);
    }
    assert_eq!(app.cfg.panel_details, [true; 6]);
    assert!(app.cfg.disk_details);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.cfg.panel_details, [false; 6]);
    assert!(!app.cfg.disk_details);
    assert_eq!(app.proc_view.selected_pid(), selected);
    assert_eq!(app.procs_shown.showing(), Some(showing));
    for (width, height) in [(0, 0), (5, 3), (80, 24)] {
      render(&mut app, width, height);
      assert_eq!(app.procs_shown.showing(), Some(showing));
      assert_eq!(app.proc_view.selected_pid(), selected);
    }
  }

  #[test]
  fn vm_summary_mixed_and_focused_views_keep_unavailable_gpu_frequency_and_temperature_explicit() {
    let soc = macmon::SocInfo {
      vm: true,
      cpu_tiers: vec![macmon::CpuTierInfo { label: "_".into(), cores: 4, freqs: Vec::new() }],
      ..Default::default()
    };
    let mut app = App::from_parts(soc, Config::default());
    app.update_metrics(macmon::Metrics {
      cpu_tiers: vec![macmon::CpuTierMetrics {
        label: "_".into(),
        cores: Vec::new(),
        freq_mhz: 0,
        scaled_ratio: 0.0,
        active_ratio: 0.63,
      }],
      ..Default::default()
    });
    for (detailed, full) in [(false, false), (true, false), (false, true)] {
      app.cfg.panel_details[Panel::Compute as usize] = detailed;
      app.panels.focus = Panel::Compute;
      app.panels.fullscreen = full;
      let view = render(&mut app, 80, 24);
      assert!(
        view.contains("CPU 63.0%")
          && view.contains("frequency unavailable")
          && view.contains("GPU unavailable"),
        "{view}"
      );
      assert!(!view.contains("0 MHz") && !view.contains("GPU 0.0%"));
    }
    app.panels.focus = Panel::Cooling;
    let view = render(&mut app, 80, 24);
    assert!(
      view.contains("CPU/GPU temperature unavailable")
        && view.contains("Raw sensor details unavailable")
    );
  }

  #[test]
  fn advanced_overview_process_mouse_restores_focus_for_rows_headers_and_wheel() {
    let mut app = app();
    render(&mut app, 200, 50);
    let showing = app.procs_shown.showing().unwrap();
    app.update_procs(
      showing,
      (0..10)
        .map(|index| ProcInfo {
          pid: 1000 + index,
          name: format!("worker{index}"),
          cpu_pct: index as f32,
          path: String::new(),
          user: "tester".into(),
          cpu_time_ns: 0,
          mem_bytes: 0,
          power_w: None,
          gpu_pct: 0.0,
          started: None,
        })
        .collect(),
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.panels.focus, Panel::Disk);
    press(&mut app, KeyCode::Char('d'));
    render(&mut app, 200, 50);
    let rect = app.layout(Rect::new(0, 0, 200, 50)).proc.unwrap();
    let mouse =
      |kind, row| MouseEvent { kind, column: rect.x + 5, row, modifiers: KeyModifiers::NONE };
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), rect.y + 2));
    assert_eq!((app.panels.focus, app.proc_view.selected_pid()), (Panel::Processes, Some(1009)));
    press(&mut app, KeyCode::Tab);
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.y + 2));
    assert_eq!((app.panels.focus, app.proc_view.selected_pid()), (Panel::Processes, Some(1006)));
    press(&mut app, KeyCode::Tab);
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), rect.y + 1));
    assert_eq!(app.panels.focus, Panel::Processes);
    assert_eq!(app.proc_view.sort, crate::config::ProcSort::Pid);
  }

  #[test]
  fn full_cooling_temporarily_shows_fans_without_changing_the_saved_detail_preference() {
    let mut app = app();
    app.cooling.sync(&[macmon::FanMetric { name: "fan0".into(), rpm: 1800, max_rpm: None }]);
    app.panels.focus = Panel::Cooling;
    render(&mut app, 80, 32);
    assert!(app.placements.is_empty());
    press(&mut app, KeyCode::Char('f'));
    render(&mut app, 80, 32);
    assert!(app.fan_frames_active() && !app.placements.is_empty());
    assert_eq!(app.cfg.panel_details, [false; 6]);
    app.cooling.frame(std::time::Duration::from_millis(100));
    let angle = app.cooling.items()[0].rotor.angle;
    press(&mut app, KeyCode::Tab);
    render(&mut app, 80, 32);
    assert!(app.placements.is_empty() && !app.fan_frames_active());
    assert!(app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)).is_continue());
    render(&mut app, 80, 32);
    assert!(!app.placements.is_empty());
    assert_eq!(app.cooling.items()[0].rotor.angle, angle);
    press(&mut app, KeyCode::Char('f'));
    render(&mut app, 80, 32);
    assert!(app.placements.is_empty() && !app.fan_frames_active());
    assert_eq!(app.cfg.panel_details, [false; 6]);
  }

  #[test]
  fn network_full_view_reaches_every_interface_and_recovers_after_resize() {
    let mut app = app();
    app.network = Some(
      (0..40)
        .map(|index| macmon::NetworkMetric {
          name: format!("utun{index}"),
          rx_bytes: index,
          tx_bytes: 0,
          rx_bytes_per_sec: None,
          tx_bytes_per_sec: None,
        })
        .collect(),
    );
    app.panels.focus = Panel::Network;
    app.panels.fullscreen = true;
    let top = render(&mut app, 80, 24);
    assert!(top.contains("utun0") && !top.contains("utun39"));
    press(&mut app, KeyCode::End);
    assert!(render(&mut app, 80, 24).contains("utun39"));
    render(&mut app, 5, 3);
    assert!(render(&mut app, 80, 24).contains("utun39"));
    press(&mut app, KeyCode::Home);
    assert!(render(&mut app, 80, 24).contains("utun0"));
  }

  #[test]
  fn cooling_full_view_scrolls_all_sensor_rows_even_after_a_tiny_window() {
    let mut app = app();
    app.temperatures.push(
      (0..40)
        .map(|index| {
          sensor(&format!("Sensor{index:02}"), TemperatureGroup::Cpu, 80.0 - index as f32)
        })
        .collect(),
      &TempMetrics { cpu_temp_avg: Some(60.5), gpu_temp_avg: None },
    );
    app.panels.focus = Panel::Cooling;
    app.panels.fullscreen = true;
    let first = render(&mut app, 80, 24);
    assert!(first.contains("CPU SMC Sensor00") && !first.contains("CPU SMC Sensor39"));
    press(&mut app, KeyCode::End);
    assert!(render(&mut app, 80, 24).contains("CPU SMC Sensor39"));
    render(&mut app, 5, 3);
    assert!(render(&mut app, 80, 24).contains("CPU SMC Sensor39"));
    press(&mut app, KeyCode::Home);
    assert!(render(&mut app, 80, 24).contains("CPU SMC Sensor00"));
  }

  #[test]
  fn temperature_details_are_current_with_gapped_bounded_mean_history_and_raw_identity() {
    let mut temperatures = Temperatures::default();
    let sensors = vec![
      sensor("Tc01", TemperatureGroup::Cpu, 40.0),
      sensor("Tc02", TemperatureGroup::Cpu, 60.0),
      sensor("Tg01", TemperatureGroup::Gpu, 45.0),
    ];
    temperatures.push(sensors, &TempMetrics { cpu_temp_avg: Some(50.0), gpu_temp_avg: Some(45.0) });
    temperatures.push(Vec::new(), &TempMetrics::default());
    let lines: String = temperatures
      .lines(60)
      .iter()
      .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
      .collect();
    assert!(lines.contains("CPU sensors 0") && lines.contains("avg 50.0°C") && lines.contains('·'));
    assert!(!lines.contains("Tc01") && temperatures.current == [None, None]);
    for _ in 0..70 {
      temperatures.push(
        vec![sensor("TcRAW", TemperatureGroup::Cpu, 55.0)],
        &TempMetrics { cpu_temp_avg: Some(55.0), gpu_temp_avg: None },
      );
    }
    assert_eq!(temperatures.history[0].len(), 60);
    let lines: String = temperatures
      .lines(60)
      .iter()
      .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
      .collect();
    assert!(
      lines.contains("CPU sensors 1")
        && lines.contains("55.0°C CPU SMC TcRAW")
        && lines.contains("not physical cores")
    );
  }

  #[test]
  fn collapsing_cooling_stops_frames_and_reopening_keeps_fan_identity() {
    let mut app = app();
    app.cooling.sync(&[macmon::FanMetric { name: "real-fan".into(), rpm: 2000, max_rpm: None }]);
    app.cfg.panel_details[Panel::Cooling as usize] = true;
    app.panels.focus = Panel::Cooling;
    app.panels.fullscreen = true;
    render(&mut app, 80, 32);
    assert!(app.fan_frames_active());
    app.cooling.frame(std::time::Duration::from_millis(100));
    let angle = app.cooling.items()[0].rotor.angle;
    press(&mut app, KeyCode::Char('5'));
    app.panels.fullscreen = false;
    render(&mut app, 80, 24);
    assert!(!app.fan_frames_active() && app.placements.is_empty());
    press(&mut app, KeyCode::Char('5'));
    app.panels.fullscreen = true;
    render(&mut app, 80, 32);
    assert_eq!(app.cooling.items()[0].rotor.angle, angle);
    assert_eq!(app.cooling.items()[0].name, "real-fan");
  }
}
