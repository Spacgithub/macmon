//! Metrics box (the metric boxes of the original macmon: CPU clusters, GPU, RAM, CPU / GPU / ANE
//! / total power) and the box frame it shares with the process list: rounded borders, titles fitted
//! on the top border, a text and the key hints on the bottom border.

use std::ops::Range;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
  Block, BorderType, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table,
};

use super::App;
use super::layout::{LayoutPlan, Metric};
use super::proc_view::{ELLIPSIS, head};
use super::store::{ClusterStore, FreqStore, PowerStore, ratio};
use super::theme::{self, dim, faint, gradient, heading, text};
use super::widgets::{Gauge, Graph};
use crate::config::{RatioMode, TUI_MAX_MS, ViewType};

/// Bytes in a GiB, the unit of the RAM box title.
const GIB: f64 = (1u64 << 30) as f64;
/// Between the key hints on a bottom border.
const SEPARATOR: &str = " | ";
/// Fewest cells worth a text on the left of a bottom border; with less room it is left out.
const BORDER_TEXT_MIN: u16 = 8;

/// Temperature position on the load gradient: 30 °C is cool, 100 °C is hot.
fn temp_ratio(celsius: f32) -> f64 {
  (f64::from(celsius) - 30.0) / 70.0
}

fn network_quantity(mut bytes: f64) -> (f64, &'static str) {
  let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let mut unit = 0;
  while bytes >= 1024.0 && unit < units.len() - 1 {
    bytes /= 1024.0;
    unit += 1;
  }
  (bytes, units[unit])
}

fn network_bytes(bytes: f64) -> String {
  let (bytes, unit) = network_quantity(bytes);
  format!("{bytes:.1} {unit}")
}

/// `width` cells as `u16`, the type of screen coordinates; saturating, as no text on screen is
/// wider than the screen.
pub(super) fn cells(width: usize) -> u16 {
  u16::try_from(width).unwrap_or(u16::MAX)
}

fn width_u16(line: &Line) -> u16 {
  cells(line.width())
}

fn spans_width(spans: &[Span]) -> u16 {
  cells(spans.iter().map(Span::width).sum())
}

// MARK: Titles

/// Titles on the top border of a box: left titles and an optional right one. When the border is
/// too short, the first left title is truncated, while later left titles and then the right
/// title are dropped, so titles never overlap.
#[derive(Default)]
pub(super) struct Titles<'a> {
  left: Vec<Line<'a>>,
  right: Option<Line<'a>>,
}

impl<'a> Titles<'a> {
  pub(super) fn new(title: impl Into<Line<'a>>) -> Self {
    Self { left: vec![title.into()], ..Default::default() }
  }

  /// Adds a title after the existing left titles.
  pub(super) fn left(mut self, title: impl Into<Line<'a>>) -> Self {
    self.left.push(title.into());
    self
  }

  pub(super) fn right(mut self, title: impl Into<Line<'a>>) -> Self {
    self.right = Some(title.into());
    self
  }

  /// Whether every title fits uncut on the top border of a box `width` cells wide.
  fn fits(&self, width: u16) -> bool {
    let padded = |line: &Line| width_u16(line).saturating_add(2);
    let left: Vec<u16> = self.left.iter().map(padded).collect();
    let right = self.right.as_ref().map(padded);
    let slots = place_titles(width, &left, right);
    slots.left.iter().map(|&(_, w)| w).eq(left.iter().copied())
      && slots.right.is_some() == right.is_some()
  }

  /// Draws the titles over the top border of `area`; unstyled text gets `style`.
  fn render(self, area: Rect, buf: &mut Buffer, style: Style) {
    let pad = |line: Line<'a>| {
      let mut spans = vec![Span::raw(" ")];
      spans.extend(line.spans);
      spans.push(Span::raw(" "));
      Line::from(spans).style(style.patch(line.style))
    };

    let left: Vec<Line> = self.left.into_iter().map(pad).collect();
    let right = self.right.map(pad);

    let widths: Vec<u16> = left.iter().map(width_u16).collect();
    let slots = place_titles(area.width, &widths, right.as_ref().map(width_u16));
    for (line, (x, width)) in left.iter().zip(slots.left) {
      buf.set_line(area.x + x, area.y, line, width);
    }

    if let (Some(line), Some(x)) = (right, slots.right) {
      buf.set_line(area.x + x, area.y, &line, width_u16(&line));
    }
  }
}

/// The first of `variants` (longest first) whose titles fit uncut on the top border of a box
/// `width` cells wide; when none does, the last one, which `Titles::render` cuts to fit.
fn fit_titles(width: u16, variants: Vec<Titles>) -> Titles {
  let mut variants = variants.into_iter().peekable();
  while let Some(titles) = variants.next() {
    if variants.peek().is_none() || titles.fits(width) {
      return titles;
    }
  }
  Titles::default()
}

/// Positions of titles on a border `width` cells wide, as offsets from the box's left edge.
#[derive(Debug, Default, PartialEq, Eq)]
struct TitleSlots {
  /// `(x, visible width)` of the left titles that fit.
  left: Vec<(u16, u16)>,
  right: Option<u16>,
}

/// Fits titles of the given widths on a border `width` cells wide. Titles keep at least one border
/// cell between each other and next to the corners. The first left title is truncated to fit; the
/// other titles are placed in full or dropped: left ones first, then the right one.
fn place_titles(width: u16, left: &[u16], right: Option<u16>) -> TitleSlots {
  let (left, free) = place_left(width, left);
  let right = right.filter(|&title| title > 0 && free.start.saturating_add(title) <= free.end);
  TitleSlots { left, right: right.map(|title| free.end - title) }
}

/// The left titles of `place_titles`: their `(x, visible width)` and the cells still free after
/// them, `[start, end)`.
fn place_left(width: u16, left: &[u16]) -> (Vec<(u16, u16)>, Range<u16>) {
  let mut slots = vec![];
  let mut free = 2..width.saturating_sub(2);
  for (i, &title) in left.iter().enumerate() {
    let room = free.end.saturating_sub(free.start);
    if room == 0 || (i > 0 && title > room) {
      break;
    }

    let title = title.min(room);
    slots.push((free.start, title));
    free.start = free.start.saturating_add(title).saturating_add(1);
  }
  (slots, free)
}

/// Cells for the text of one more left title after `titles` on the top border of a box `width`
/// cells wide, as `place_titles` places them: the cells still free, without the blank cell on both
/// sides of the text.
pub(super) fn title_text_room(width: u16, titles: &[Line]) -> u16 {
  let padded: Vec<u16> = titles.iter().map(|title| width_u16(title).saturating_add(2)).collect();
  let (_, free) = place_left(width, &padded);
  free.end.saturating_sub(free.start).saturating_sub(2)
}

/// Draws a rounded box with `titles` on the top border. Returns the area inside the borders.
pub(super) fn draw_box(f: &mut Frame, area: Rect, titles: Titles) -> Rect {
  let block = Block::bordered().border_type(BorderType::Rounded).border_style(theme::BORDER);
  let inner = block.inner(area);
  f.render_widget(block, area);
  // title text without a color of its own in the text color, not the border's
  titles.render(area, f.buffer_mut(), Style::new().fg(theme::TEXT));
  inner
}

/// Draws a divider across box `area` at row `y`, `├──┤` in the border color, with `titles` on it
/// as on the top border.
pub(super) fn draw_divider(f: &mut Frame, area: Rect, y: u16, titles: Titles) {
  let line = format!("├{}┤", "─".repeat(usize::from(area.width.saturating_sub(2))));
  f.buffer_mut().set_string(area.x, y, line, theme::BORDER);
  titles.render(Rect { y, height: 1, ..area }, f.buffer_mut(), Style::new().fg(theme::TEXT));
}

/// Draws a scrollbar over the right border of box `area` beside `rows`, the cells of a list of
/// `len` rows scrolled to `offset`, when they don't fit: `↑` and `↓` at the ends, the thumb
/// between them on the border line. Fewer than 3 rows leave no room for it.
pub(super) fn draw_scrollbar(f: &mut Frame, area: Rect, rows: Rect, len: usize, offset: usize) {
  let page = usize::from(rows.height);
  if len <= page || page < 3 || area.width < 2 {
    return;
  }
  let bar = Rect { x: area.right() - 1, width: 1, ..rows };
  // scroll positions, not rows, so the thumb reaches the bottom at the last one
  let mut state =
    ScrollbarState::new(len - page + 1).position(offset).viewport_content_length(page);
  let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
    .begin_symbol(Some("↑"))
    .end_symbol(Some("↓"))
    .track_symbol(None)
    .thumb_symbol("┃")
    .style(theme::BORDER)
    .thumb_style(theme::TEXT);
  f.render_stateful_widget(scrollbar, bar, &mut state);
}

// MARK: Bottom border

/// A key hint: the key in `theme::key`, what it does `theme::faint` around it. A letter key in the
/// label stays in it (the first match: `quit`), any other key comes before it (`/ filter`, `Enter
/// keep`); with an empty label only the key shows (`?`).
pub(super) fn hint(key: &'static str, label: impl Into<String>) -> Vec<Span<'static>> {
  let label = label.into();
  let letter = key.as_bytes().first().filter(|c| key.len() == 1 && c.is_ascii_alphabetic());
  if let Some(i) = letter.and_then(|c| label.bytes().position(|b| b.eq_ignore_ascii_case(c))) {
    let (head, tail) = (label[..i].to_string(), label[i + 1..].to_string());
    return vec![faint(head), theme::key(label[i..=i].to_string()), faint(tail)];
  }
  if label.is_empty() {
    return vec![theme::key(key)];
  }
  vec![theme::key(key), faint(format!(" {label}"))]
}

/// `items` joined by a dim ` | `.
pub(super) fn join(items: impl IntoIterator<Item = Vec<Span<'static>>>) -> Vec<Span<'static>> {
  let mut spans = vec![];
  for (i, item) in items.into_iter().enumerate() {
    if i > 0 {
      spans.push(dim(SEPARATOR));
    }
    spans.extend(item);
  }
  spans
}

/// `spans` with a blank cell at both ends.
fn padded(spans: Vec<Span<'static>>) -> Line<'static> {
  let mut line = vec![Span::raw(" ")];
  line.extend(spans);
  line.push(Span::raw(" "));
  Line::from(line)
}

/// Number of leading `items` (by width) that fit in `room` cells joined by ` | `, with a blank
/// cell at both ends.
fn fit_joined(room: u16, items: &[u16]) -> usize {
  // the blank cells at both ends, then each item after its separator
  let mut used = 2;
  for (i, &item) in items.iter().enumerate() {
    let gap = if i == 0 { 0 } else { SEPARATOR.len() as u32 };
    used += gap + u32::from(item);
    if used > u32::from(room) {
      return i;
    }
  }

  items.len()
}

/// `spans` cut to `max` cells, ending with a dim `…` when cut (right after the last word kept).
fn cut_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
  if spans.iter().map(Span::width).sum::<usize>() <= max {
    return spans;
  }

  let mut room = max.saturating_sub(1);
  let mut cut = vec![];
  for span in spans {
    let kept = head(&span.content, room);
    if kept.len() < span.content.len() {
      cut.push(Span::styled(kept.trim_end().to_string(), span.style));
      break;
    }
    room -= Span::raw(kept).width();
    cut.push(span);
  }
  cut.push(dim(ELLIPSIS));
  cut
}

/// Draws `hints` (right-aligned) and a text (left) over the bottom border of box `area`. Like
/// titles, they keep a border cell next to the corners and between each other. The hints come
/// first and drop from the end when the border is too short, so `quit` stays as long as it fits.
/// The text gets the cells left: `text` makes it for that many cells (without its blank cells), and
/// it is cut with `…` when longer, or left out with fewer than `BORDER_TEXT_MIN`.
pub(super) fn render_bottom_border(
  f: &mut Frame,
  area: Rect,
  hints: Vec<Vec<Span<'static>>>,
  text: impl FnOnce(usize) -> Vec<Span<'static>>,
) {
  let y = area.bottom() - 1;
  let mut free = area.width.saturating_sub(4);

  let widths: Vec<u16> = hints.iter().map(|hint| spans_width(hint)).collect();
  let shown = fit_joined(free, &widths);
  if shown > 0 {
    let line = padded(join(hints.into_iter().take(shown)));
    let width = width_u16(&line);
    f.buffer_mut().set_line(area.right() - 2 - width, y, &line, width);
    free = free.saturating_sub(width + 1);
  }

  let room = free.saturating_sub(2);
  if room >= BORDER_TEXT_MIN {
    let spans = cut_spans(text(usize::from(room)), usize::from(room));
    if !spans.is_empty() {
      f.buffer_mut().set_line(area.x + 2, y, &padded(spans), room + 2);
    }
  }
}

// MARK: Metric boxes

/// How a metric box draws its history.
enum Scale {
  /// A load (CPU cluster, GPU, RAM): the graph is scaled to `max` and colored by load, and `v`
  /// switches it to a gauge of the current `load` (`0.0..=1.0`).
  Load { max: u64, load: f64 },
  /// Power: the graph is scaled to its largest visible sample, in the low load color, as in the
  /// original; no gauge.
  Power,
}

/// Titles and graph of one metric box.
struct MetricBox<'a> {
  /// Title variants, longest first; the box shows the first that fits (see `fit_titles`).
  titles: Vec<Titles<'static>>,
  /// History, newest first.
  data: &'a [u64],
  scale: Scale,
}

impl App {
  pub(super) fn render_network_box(&self, f: &mut Frame, area: Rect) {
    let metrics = self.network.as_deref().unwrap_or_default();
    let shown = usize::from(area.height.saturating_sub(3)).min(3).min(metrics.len());
    let title =
      vec![theme::value("Network", theme::SECTION), dim(format!(" {shown}/{}", metrics.len()))];
    let inner = draw_box(f, area, Titles::new(title));
    if metrics.is_empty() {
      let status = if self.network.is_none() { "unavailable" } else { "no active interfaces" };
      f.render_widget(Paragraph::new(dim(status)), inner);
      return;
    }
    let mut widths = [15u16, 12, 12, 11, 11];
    let headers = ["Interface", "↓ RX/s", "↑ TX/s", "RX total", "TX total"];
    let mut columns = widths.len();
    while columns > 1 && widths[..columns].iter().sum::<u16>() + columns as u16 - 1 > inner.width {
      columns -= 1;
    }
    let needed = widths[..columns].iter().sum::<u16>() + columns as u16 - 1;
    let table = if inner.width >= needed + 2 { inner.inner(Margin::new(1, 0)) } else { inner };
    widths[0] += table.width.saturating_sub(needed);
    let rate = |v: Option<f64>, color| {
      let line = match v {
        Some(v) if v > 0.0 => {
          let (value, unit) = network_quantity(v);
          Line::from(vec![theme::value(format!("{value:.1}"), color), dim(format!(" {unit}/s"))])
        }
        v => Line::from(dim(v.map_or_else(|| "—".into(), |v| format!("{}/s", network_bytes(v))))),
      };
      Cell::from(line.right_aligned())
    };
    let rows = metrics.iter().take(shown).map(|m| {
      let values = [
        Cell::from(
          if m.rx_bytes_per_sec.unwrap_or(0.0) + m.tx_bytes_per_sec.unwrap_or(0.0) > 0.0 {
            heading(m.name.clone())
          } else {
            text(m.name.clone())
          },
        ),
        rate(m.rx_bytes_per_sec, theme::DOWNLOAD),
        rate(m.tx_bytes_per_sec, theme::UPLOAD),
        Cell::from(Line::from(dim(network_bytes(m.rx_bytes as f64))).right_aligned()),
        Cell::from(Line::from(dim(network_bytes(m.tx_bytes as f64))).right_aligned()),
      ];
      Row::new(values.into_iter().take(columns))
    });
    let widths = widths[..columns].iter().copied().map(Constraint::Length);
    let header = Row::new(headers.into_iter().take(columns).enumerate().map(|(i, label)| {
      let color = match i {
        1 => theme::DOWNLOAD,
        2 => theme::UPLOAD,
        _ => theme::DIM,
      };
      let line = Line::from(Span::styled(label, color));
      if i == 0 { line } else { line.right_aligned() }
    }));
    f.render_widget(
      Table::new(rows, widths).header(header).style(Style::new().fg(theme::TEXT)),
      table,
    );
  }

  /// `load` (`0.0..=1.0`) as a percent with `decimals`, in its load color.
  fn percent(&self, load: f64, decimals: usize) -> Span<'static> {
    theme::value(format!("{:.decimals$}%", load * 100.0), gradient(load))
  }

  /// Key hints for the bottom border of the lowest box (see `hint`): the global keys, help first as
  /// a lone `?` (hints drop from the end), with the ratio mode and the interval (`? | quit | procs
  /// | view | freq | -/+  1000ms`; no `procs` while the window is too small for the process list),
  /// or the filter keys while a filter is typed (`Enter keep | Esc clear`). The mode
  /// and the interval are padded to their widest value, so a toggle doesn't move the hints left of
  /// them.
  pub(super) fn footer_hints(&self) -> Vec<Vec<Span<'static>>> {
    if self.proc_view.typing() {
      return vec![hint("Enter", "keep"), hint("Esc", "clear")];
    }

    let mut hints = vec![hint("?", ""), hint("q", "quit")];
    if self.procs_fit {
      hints.push(hint("p", "procs"));
    }
    let modes = [RatioMode::Scaled, RatioMode::Active].map(|mode| mode.label().len());
    let mode = format!("{:1$}", self.cfg.ratio_mode.label(), modes.into_iter().max().unwrap_or(0));
    let interval = format!("{:>1$}ms", self.cfg.interval(), TUI_MAX_MS.to_string().len());
    hints.extend([
      hint("v", "view"),
      hint("r", mode),
      hint("a", if self.cfg.animate_fans { "animate" } else { "still" }),
      hint("-/+", interval),
    ]);
    hints
  }

  /// Metrics box: chip and version in the title, the metric boxes inside and the key hints on the
  /// bottom border when it is the lowest box.
  pub(super) fn render_metrics_box(&self, f: &mut Frame, plan: &LayoutPlan) {
    let Some(area) = plan.top else { return };
    draw_box(f, area, self.metrics_titles(area.width));

    if let Some(compute) = plan.compute {
      let mode = if self.cfg.view_type == ViewType::Gauge { "gauge" } else { "history" };
      draw_box(f, compute, Titles::new(theme::value("Compute", theme::CPU)).right(dim(mode)));
    }

    for &(kind, r) in &plan.boxes {
      let metric = self.metric_box(kind, r.width);
      let overview = kind == Metric::Ram && plan.compute.is_some();
      let titles = if overview {
        let load = ratio(self.mem.ram_usage as f64, self.mem.ram_total as f64);
        let mut title =
          vec![theme::value("Memory", theme::MEMORY), text(" "), self.percent(load, 0)];
        if self.mem.swap_total > 0 {
          title.extend([
            dim(" · SWAP "),
            self.percent(ratio(self.mem.swap_usage as f64, self.mem.swap_total as f64), 0),
          ]);
        }
        Titles::new(title)
      } else {
        fit_titles(r.width, metric.titles)
      };
      let mut inner = draw_box(f, r, titles);
      if overview && inner.height > 1 {
        let quantity = vec![
          theme::value(format!("{:.2}", self.mem.ram_usage as f64 / GIB), theme::MEMORY),
          dim(format!(" / {:.1} GiB", self.mem.ram_total as f64 / GIB)),
        ];
        f.render_widget(Paragraph::new(Line::from(quantity)), Rect { height: 1, ..inner });
        inner.y += 1;
        inner.height -= 1;
      }
      let accent = match kind {
        Metric::Cluster(_) => theme::CPU,
        Metric::Gpu => theme::GPU,
        Metric::Ram => theme::MEMORY,
        _ => theme::POWER,
      };
      if r.width > 1 && r.height > 2 {
        let stripe = Rect { y: r.y + 1, width: 1, height: r.height - 2, ..r };
        f.buffer_mut().set_style(stripe, Style::new().fg(accent));
      }
      let graph_area =
        inner.inner(Margin::new(u16::from(inner.width > 4), u16::from(inner.height > 3)));
      let graph =
        Graph::new(metric.data).color(accent).emphasis().three_levels(self.three_level_bars);
      match metric.scale {
        Scale::Load { load, .. } if self.cfg.view_type == ViewType::Gauge => {
          f.render_widget(Gauge::new(load), inner)
        }
        Scale::Load { max, .. } => f.render_widget(graph.accent(accent).max(max), graph_area),
        Scale::Power => f.render_widget(graph, graph_area),
      }
    }

    if let Some(telemetry) = plan.telemetry {
      self.render_telemetry(f, telemetry);
    }
    if let Some(area) = self.fan_area(plan) {
      self.render_fan(f, area);
    }

    if let Some(area) = plan.network {
      self.render_network_box(f, area);
    }

    let hints = if plan.proc.is_none() { self.footer_hints() } else { vec![] };
    render_bottom_border(f, area, hints, |_| vec![]);
  }

  /// Brand and hardware on the left, version on the right. Components drop whole as needed.
  fn metrics_titles(&self, width: u16) -> Titles<'static> {
    let chip = self.chip_title();
    let mut branded = chip.clone();
    if !self.soc.chip_name.is_empty() {
      branded.spans.splice(0..0, [theme::value("MACMON", theme::SECTION), dim(" / ")]);
    }
    fit_titles(
      width,
      vec![
        Titles::new(branded.clone()).right(dim(format!("v{}", env!("CARGO_PKG_VERSION")))),
        Titles::new(branded),
        Titles::new(chip),
        Titles::new(heading(self.chip_name().to_owned())),
        Titles::default(),
      ],
    )
  }

  fn chip_name(&self) -> &str {
    if self.soc.chip_name.is_empty() { env!("CARGO_PKG_NAME") } else { &self.soc.chip_name }
  }

  /// `Apple M3 Pro (6E+6P+18GPU 36GB)`, the details in parentheses dim. Core counts come from the
  /// CPU clusters, so a third tier shows up as `6E+4P+2S`.
  fn chip_title(&self) -> Line<'static> {
    let soc = &self.soc;
    // a tier of unknown core type `_` (a VM's single tier) reads `4CPU`, like `18GPU`
    let unit = |c: &ClusterStore| match c.label.as_str() {
      "_" => format!("{}CPU", c.count),
      label => format!("{}{label}", c.count),
    };
    let mut units: Vec<String> = self.clusters.items.iter().map(unit).collect();
    if soc.gpu_cores > 0 {
      units.push(format!("{}GPU", soc.gpu_cores));
    }

    let mut details = units.join("+");
    if soc.memory_gb > 0 {
      if !details.is_empty() {
        details.push(' ');
      }
      details.push_str(&format!("{}GB", soc.memory_gb));
    }

    // a chip name the library couldn't read still leaves a title
    let mut spans = vec![heading(self.chip_name().to_string())];
    if !details.is_empty() {
      spans.push(dim(format!(" ({details})")));
    }
    Line::from(spans)
  }

  /// Titles and history of `metric`; its cluster comes from the same list the layout counts.
  fn metric_box(&self, metric: Metric, width: u16) -> MetricBox<'_> {
    match metric {
      Metric::Cluster(i) => {
        let cluster = &self.clusters.items[i];
        // a single tier (a VM) is just the CPU
        let label = match self.clusters.items.len() {
          1 => "CPU".to_string(),
          _ => format!("{}-CPU", cluster.label),
        };
        self.freq_box(label, &cluster.freq)
      }
      Metric::Gpu => self.freq_box("GPU".to_string(), &self.igpu_freq),
      Metric::Ram => self.ram_box(),
      Metric::CpuPower => self.power_box("CPU", &self.cpu_power, self.cpu_temp.last(), width),
      Metric::GpuPower => self.power_box("GPU", &self.gpu_power, self.gpu_temp.last(), width),
      Metric::AnePower => self.power_box("ANE", &self.ane_power, None, width),
      Metric::TotalPower => self.total_power_box(width),
    }
  }

  /// `E-CPU 42% @ 1800 MHz` (the percent colored by load), or `E-CPU 42%` when the frequency
  /// doesn't fit or is 0 (none read), over the usage history scaled to 100 %, or a gauge.
  fn freq_box<'a>(&self, label: String, freq: &'a FreqStore) -> MetricBox<'a> {
    let series = freq.ratio(self.cfg.ratio_mode);
    let short = vec![heading(label), text(" "), self.percent(series.ratio, 0)];
    let mut full = short.clone();
    if freq.freq_mhz > 0 {
      full.push(dim(format!(" @ {} MHz", freq.freq_mhz)));
    }

    MetricBox {
      titles: vec![Titles::new(full), Titles::new(short)],
      data: &series.items,
      scale: Scale::Load { max: 100, load: series.ratio },
    }
  }

  /// RAM and swap usage over the RAM usage history scaled to the total RAM, or a gauge. The total
  /// RAM is in the chip title, so the title shows what is used: `RAM 16.81 GB (70.0%) · SWAP 2.37
  /// / 3.0 GB`, or the percentages `RAM 70% · SWAP 79%` / `RAM 70% SW 79%` when that doesn't fit
  /// (without swap only the RAM part). Narrower, the swap part drops whole (`RAM 70%`), then the
  /// title, so no number is ever cut.
  fn ram_box(&self) -> MetricBox<'_> {
    let mem = &self.mem;
    let gib = |bytes: u64| bytes as f64 / GIB;
    let (used, load) = (gib(mem.ram_usage), ratio(gib(mem.ram_usage), gib(mem.ram_total)));
    let full = [
      heading("RAM"),
      text(" "),
      theme::value(format!("{used:.2}"), theme::MEMORY),
      dim(" GB ("),
      self.percent(load, 1),
      dim(")"),
    ];
    let short = [heading("RAM"), text(" "), self.percent(load, 0)];

    let mut titles = vec![];
    if mem.swap_total > 0 {
      let (used, total) = (gib(mem.swap_usage), gib(mem.swap_total));
      let swap = [dim(" · "), heading("SWAP")];
      let full_swap = [text(format!(" {used:.2} / {total:.1} GB"))];
      let short_swap = [text(" "), self.percent(ratio(used, total), 0)];
      titles.push([&full[..], &swap, &full_swap].concat());
      titles.push([&short[..], &swap, &short_swap].concat());
      titles.push([&short[..], &[text(" "), heading("SW")], &short_swap].concat());
    } else {
      titles.push(full.into());
    }
    titles.push(short.into());

    let mut titles: Vec<Titles> = titles.into_iter().map(Titles::new).collect();
    titles.push(Titles::default());
    MetricBox { titles, data: &mem.items, scale: Scale::Load { max: mem.ram_total, load } }
  }

  /// `CPU 4.50W (3.10, 8.20)` (current, average, maximum; original format), or `CPU 4.50W` when
  /// that doesn't fit, with the temperature (`45°C`) on the right when the sensor exists, over the
  /// power history, scaled to its largest visible sample. Narrow boxes reduce precision before
  /// dropping the value, so the number and its unit are never cut. Always a graph.
  fn power_box<'a>(
    &self,
    label: &'static str,
    store: &'a PowerStore,
    temp: Option<f32>,
    width: u16,
  ) -> MetricBox<'a> {
    let value = |precision| {
      vec![
        heading(label),
        text(" "),
        theme::value(format!("{:.*}", precision, store.top_value), theme::POWER),
        dim("W"),
      ]
    };
    let short = value(2);
    let mut full = short.clone();
    full.push(dim(format!(" ({:.2}, {:.2})", store.avg_value, store.max_value)));

    let mut titles: Vec<_> = [full, short]
      .into_iter()
      .map(|left| {
        let titles = Titles::new(left);
        match temp {
          Some(temp) => {
            titles.right(Span::styled(format!("{temp:.0}°C"), gradient(temp_ratio(temp))))
          }
          None => titles,
        }
      })
      .collect();
    let fits = |titles: &[Titles]| titles.last().is_some_and(|title| title.fits(width));
    for precision in [2, 1, 0] {
      if fits(&titles) {
        break;
      }
      titles.push(Titles::new(value(precision)));
    }
    if !fits(&titles) {
      let compact = match label {
        "Total" => "Tot",
        "Power" => "Pwr",
        label => label,
      };
      titles.push(Titles::new(vec![
        heading(compact),
        text(" "),
        theme::value(format!("{:.0}", store.top_value), theme::POWER),
        dim("W"),
      ]));
    }
    if !fits(&titles) {
      titles.push(Titles::new(heading(label)));
    }
    if !fits(&titles) {
      titles.push(Titles::default());
    }

    MetricBox { titles, data: &store.items, scale: Scale::Power }
  }

  /// `Total 12.00W (12.00, 12.00)`, the whole system, or `Power 6.60W (6.60, 6.60)`, CPU + GPU +
  /// ANE, without the system power sensor. The fans go on its bottom border (`render_metrics_box`).
  fn total_power_box(&self, width: u16) -> MetricBox<'_> {
    // the newest sample, not the smoothed `top_value`, so the box switches as soon as it reads 0
    if self.sys_power.items.first().is_some_and(|&mw| mw > 0) {
      self.power_box("Total", &self.sys_power, None, width)
    } else {
      self.power_box("Power", &self.all_power, None, width)
    }
  }

  /// Wide console ledger: the existing total-power policy and real temperature readings.
  fn render_telemetry(&self, f: &mut Frame, area: Rect) {
    let (label, total) = if self.sys_power.items.first().is_some_and(|&mw| mw > 0) {
      ("Total", &self.sys_power)
    } else {
      ("CPU+GPU+ANE", &self.all_power)
    };
    let right = vec![
      dim(format!("{label} ")),
      theme::value(format!("{:.2}", total.top_value), theme::POWER),
      dim(" W"),
    ];
    let inner = draw_box(f, area, Titles::new(theme::value("Power", theme::POWER)).right(right));
    let quantity = |value: f64| {
      Line::from(vec![theme::value(format!("{value:.2}"), theme::POWER), dim(" W")]).right_aligned()
    };
    let mut rows: Vec<Row> =
      [("CPU", &self.cpu_power), ("GPU", &self.gpu_power), ("ANE", &self.ane_power)]
        .into_iter()
        .map(|(label, store)| Row::new([Line::from(dim(label)), quantity(store.top_value)]))
        .collect();
    rows.push(Row::new([Line::from(heading("Temperature")), Line::default()]));
    for (label, temp) in [("CPU", self.cpu_temp.last()), ("GPU", self.gpu_temp.last())] {
      let value = temp.map_or_else(
        || dim("—"),
        |value| Span::styled(format!("{value:.0}°C"), gradient(temp_ratio(value))),
      );
      rows.push(Row::new([Line::from(dim(label)), Line::from(value).right_aligned()]));
    }
    f.render_widget(
      Table::new(rows, [Constraint::Min(14), Constraint::Length(12)]),
      inner.inner(Margin::new(1, 0)),
    );
  }

  pub(super) fn fan_area(&self, plan: &LayoutPlan) -> Option<Rect> {
    plan
      .telemetry
      .or_else(|| plan.boxes.iter().find(|(m, _)| *m == Metric::TotalPower).map(|(_, r)| *r))
      .filter(|area| area.height >= 2)
  }

  fn render_fan(&self, f: &mut Frame, area: Rect) {
    let label = self.fans.label();
    if label.is_empty() {
      return;
    }
    let mut spans = vec![];
    if self.fan_motion_visible {
      let glyph = if self.fan_phase { '╳' } else { '╋' };
      let color = if self.fans.spinning() { theme::POWER } else { theme::DIM };
      spans.extend([Span::styled(glyph.to_string(), color), text(" ")]);
    }
    spans.push(dim(label));
    render_bottom_border(f, area, vec![spans], |_| vec![]);
  }
}

#[cfg(test)]
mod tests {
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::buffer::Buffer;
  use ratatui::layout::Rect;
  use ratatui::style::{Color, Modifier, Style};
  use ratatui::text::Span;

  use super::{
    Titles, draw_box, draw_scrollbar, fit_titles, hint, place_titles, render_bottom_border,
  };
  use crate::tui::theme::{self, dim, text};

  #[test]
  fn a_scrollbar_needs_3_rows() {
    // 100 rows, 1 to 3 of them on screen: the plain border, then `↑`, a thumb and `↓`
    for height in 1..=3 {
      let mut term = Terminal::new(TestBackend::new(10, height + 2)).unwrap();
      let frame = term.draw(|f| {
        draw_box(f, f.area(), Titles::default());
        draw_scrollbar(f, f.area(), Rect::new(1, 1, 8, height), 100, 0);
      });
      let buf = frame.unwrap().buffer;
      let border: String = (1..=height).map(|y| buf[(9, y)].symbol()).collect();
      let shown = if height < 3 { "│".repeat(height.into()) } else { "↑┃↓".into() };
      assert_eq!(border, shown, "{height} rows");
    }
  }

  /// Top border of a box `width` cells wide with `titles` drawn on it.
  fn border_with(titles: Titles, width: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
    buf.set_string(0, 0, "─".repeat(width.into()), Style::new());
    titles.render(buf.area, &mut buf, Style::new());
    (0..width).map(|x| buf[(x, 0)].symbol()).collect()
  }

  #[test]
  fn fit_titles_picks_the_longest_variant_that_fits() {
    let (full, short) = ("CPU 4.50W (4.50, 4.50)", "CPU 4.50W");
    let variants = || vec![Titles::new(full).right("45°C"), Titles::new(full), Titles::new(short)];
    let fitted = |width: u16| border_with(fit_titles(width, variants()), width);
    let dashes = |n: usize| "─".repeat(n);

    assert_eq!(fitted(40), format!("── {full} {} 45°C ──", dashes(6)));
    assert_eq!(fitted(35), format!("── {full} ─ 45°C ──"));
    // the right title goes first, then the left one gets shorter
    assert_eq!(fitted(34), format!("── {full} {}", dashes(8)));
    assert_eq!(fitted(28), format!("── {full} ──"));
    assert_eq!(fitted(27), format!("── {short} {}", dashes(14)));
    assert_eq!(fitted(15), format!("── {short} ──"));
    // none fits: the last one is cut
    assert_eq!(fitted(14), "── CPU 4.50W──");
    assert_eq!(fitted(10), "── CPU 4──");
  }

  #[test]
  fn titles_never_overlap() {
    let sizes = [0, 1, 3, 6, 9, 14, 20, 33];
    for width in 0..=120 {
      for (a, b) in sizes.iter().flat_map(|a| sizes.map(|b| (*a, b))) {
        for right in [None, Some(a), Some(b)] {
          let slots = place_titles(width, &[a, b], right);
          let mut spans: Vec<(u16, u16)> = slots.left.clone();
          spans.extend(slots.right.zip(right));

          let ctx = format!("width {width} left [{a}, {b}] right {right:?}");
          for (i, &(x, w)) in spans.iter().enumerate() {
            assert!(x >= 2 && x + w + 2 <= width, "{ctx}: ({x}, {w}) off the border");
            for &(x2, w2) in &spans[i + 1..] {
              assert!(x + w < x2 || x2 + w2 < x, "{ctx}: ({x}, {w}) touches ({x2}, {w2})");
            }
          }
        }
      }
    }
  }

  /// `spans` as text with the keys (`theme::KEY`, no modifier) in brackets and the faint text
  /// (default color, `Modifier::DIM`) plain; any other style panics.
  fn marked(spans: &[Span]) -> String {
    let mark = |span: &Span| match (span.style.fg, span.style.add_modifier) {
      (Some(theme::KEY), modifier) if modifier.is_empty() => format!("[{}]", span.content),
      (Some(theme::TEXT), Modifier::DIM) => span.content.to_string(),
      style => panic!("{:?} in {style:?}", span.content),
    };
    spans.iter().map(mark).collect()
  }

  #[test]
  fn hint_marks_the_key_in_the_label_or_before_it() {
    let hints = [
      (hint("q", "quit"), "[q]uit"),
      (hint("k", "Kill"), "[K]ill"),
      (hint("s", "sessions"), "[s]essions"),
      (hint("v", "graph"), "[v] graph"),
      (hint("r", "freq"), "f[r]eq"),
      (hint("?", "help"), "[?] help"),
      (hint("Enter", "keep"), "[Enter] keep"),
      (hint("-/+", "1000ms"), "[-/+] 1000ms"),
      // an empty label: only the key, no blank after it
      (hint("?", ""), "[?]"),
    ];
    for (spans, text) in hints {
      assert_eq!(marked(&spans), text);
    }
    assert_eq!(hint("?", "").len(), 1);
  }

  #[test]
  fn bottom_border_hints_come_first_then_the_text() {
    let draw = |width| {
      let hints = vec![hint("?", "help"), hint("q", "quit")];
      let power = |_: usize| vec![text("Power 6.60W"), dim(" (6.60, 6.60)")];
      let mut term = Terminal::new(TestBackend::new(width, 1)).unwrap();
      term.draw(|f| render_bottom_border(f, f.area(), hints, power)).unwrap().buffer.clone()
    };
    let border = |width| {
      let buf = draw(width);
      (0..width).map(|x| buf[(x, 0)].symbol()).collect::<String>().replace(' ', "_")
    };
    let blank = |cells: usize| "_".repeat(cells);
    let help = "_?_help_|_quit_";

    // the hints right-aligned, the text left, a border cell next to the corners
    assert_eq!(border(60), format!("___Power_6.60W_(6.60,_6.60)_{}{help}__", blank(15)));
    // the keys cyan, the labels faint in the default color, the separators dim, nothing bold or
    // underlined
    let buf = draw(60);
    let at = |x: u16| (buf[(x, 0)].fg, buf[(x, 0)].modifier);
    let cells_with = |style| (43..58).filter(|&x| at(x) == style).collect::<Vec<_>>();
    assert_eq!(cells_with((theme::KEY, Modifier::empty())), [44, 53]);
    assert_eq!(cells_with((theme::TEXT, Modifier::DIM)), [45, 46, 47, 48, 49, 54, 55, 56]);
    assert_eq!(cells_with((theme::DIM, Modifier::empty())), [50, 51, 52]);
    assert_eq!(theme::DIM, Color::DarkGray);
    assert!(buf.content.iter().all(|cell| Modifier::DIM.contains(cell.modifier)));
    // the text is cut with `…` in the room the hints leave, and left out with fewer than 8 cells
    assert_eq!(border(40), format!("___Power_6.60W_(6.60…__{help}__"));
    assert_eq!(border(30), format!("___Power_6…__{help}__"));
    assert_eq!(border(29), format!("{}{help}__", blank(12)));
    // hints drop from the end, the first one stays as long as it fits
    assert_eq!(border(19), format!("__{help}__"));
    assert_eq!(border(18), format!("{}_?_help___", blank(8)));
    assert_eq!(border(12), "___?_help___");
    assert_eq!(border(11), blank(11));
  }
}
