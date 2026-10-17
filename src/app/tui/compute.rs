//! Reported compute activity, with raw core coverage kept separate from frequency availability.

use std::collections::HashSet;

use macmon::{CpuCoreMetrics, Metrics, SocInfo};
use ratatui::prelude::*;

use super::App;
use super::theme::{self, dim, text};
use crate::config::{RatioMode, ViewType};

#[derive(Debug)]
struct Tier {
  label: String,
  installed: usize,
  cores: Vec<CpuCoreMetrics>,
  active: f32,
  weighted: f32,
  frequency: u32,
}

#[derive(Debug)]
struct Sample {
  active: [f32; 2],
  weighted: [f32; 2],
  temperatures: [Option<f32>; 2],
  power: [f32; 2],
  tiers: Vec<Tier>,
  cpu_residency: bool,
  gpu_available: bool,
  gpu_frequency: u32,
}

/// Only the latest real core rows are retained; no installed-but-unreported cores are synthesized.
#[derive(Debug, Default)]
pub(super) struct Compute {
  latest: Option<Sample>,
  history: Vec<[Option<f32>; 4]>,
}

fn ratio(value: f32) -> Option<f32> {
  (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

fn percent(value: Option<f32>) -> String {
  value.map_or_else(|| "Unavailable".into(), |value| format!("{:.1}%", value * 100.0))
}

fn frequency(value: u32, supported: bool) -> String {
  if supported && value > 0 { format!("{value} MHz") } else { "Unavailable".into() }
}

/// Wrap complete fields, so narrow detail cards never cut a number from its unit.
fn fields(lines: &mut Vec<Line<'static>>, values: &[String], width: usize) {
  let mut current = String::new();
  for value in values {
    if value.chars().count() > width {
      if !current.is_empty() {
        lines.push(Line::from(text(std::mem::take(&mut current))));
      }
      for word in value.split_whitespace() {
        let next = if current.is_empty() { word.into() } else { format!("{current} {word}") };
        if next.chars().count() > width && !current.is_empty() {
          lines.push(Line::from(text(std::mem::take(&mut current))));
        }
        if current.is_empty() {
          current = word.into();
        } else {
          current.push(' ');
          current.push_str(word);
        }
      }
      continue;
    }
    let next = if current.is_empty() { value.clone() } else { format!("{current} · {value}") };
    if next.chars().count() > width && !current.is_empty() {
      lines.push(Line::from(text(std::mem::take(&mut current))));
      current = value.clone();
    } else {
      current = next;
    }
  }
  if !current.is_empty() {
    lines.push(Line::from(text(current)));
  }
}

fn fit_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
  lines
    .into_iter()
    .flat_map(|line| {
      if line.width() <= width {
        return vec![line];
      }
      let style = line.style.patch(line.spans.first().map_or(Style::default(), |span| span.style));
      let mut wrapped = Vec::new();
      fields(&mut wrapped, &[line.to_string()], width);
      wrapped.into_iter().map(|line| Line::from(Span::styled(line.to_string(), style))).collect()
    })
    .collect()
}

impl Compute {
  pub(super) fn push(&mut self, metrics: &Metrics, soc: &SocInfo) {
    let tiers: Vec<_> = metrics
      .cpu_tiers
      .iter()
      .map(|tier| Tier {
        label: tier.label.clone(),
        installed: soc
          .cpu_tiers
          .iter()
          .find(|info| info.label == tier.label)
          .map_or(0, |info| usize::from(info.cores)),
        cores: tier.cores.clone(),
        active: tier.active_ratio,
        weighted: tier.scaled_ratio,
        frequency: tier.freq_mhz,
      })
      .collect();
    let complete = !tiers.is_empty()
      && tiers.len() == soc.cpu_tiers.len()
      && tiers.iter().map(|tier| &tier.label).collect::<HashSet<_>>().len() == tiers.len()
      && tiers.iter().all(|tier| {
        let ids: HashSet<_> = tier.cores.iter().map(|core| (core.die_id, core.core_id)).collect();
        tier.installed > 0
          && tier.cores.len() == tier.installed
          && ids.len() == tier.installed
          && soc.cpu_tiers.iter().find(|info| info.label == tier.label).is_some_and(|info| {
            !info.freqs.is_empty() && info.freqs.iter().all(|frequency| *frequency > 0)
          })
          && ratio(tier.active).is_some()
          && ratio(tier.weighted).is_some()
          && tier.cores.iter().all(|core| {
            ratio(core.active_ratio).is_some()
              && ratio(core.scaled_ratio).is_some()
              && (core.freq_mhz > 0 || core.active_ratio == 0.0)
          })
      });
    // Tick fallback copies active into weighted and floors tier MHz to the SoC minimum. Neither
    // the tier MHz nor equal ratios proves residency exists. Complete multi-tier samples cannot
    // use that fallback; complete single-tier all-zero core clocks remain ambiguous.
    let cpu_residency = !soc.vm
      && complete
      && (tiers.len() > 1
        || tiers.iter().flat_map(|tier| &tier.cores).any(|core| core.freq_mhz > 0));
    let gpu_available = !soc.vm && soc.gpu_freqs.iter().skip(1).any(|frequency| *frequency > 0);
    let sample = Sample {
      active: [metrics.cpu_active_ratio, metrics.gpu_active_ratio],
      weighted: [metrics.cpu_scaled_ratio, metrics.gpu_scaled_ratio],
      temperatures: [metrics.temp.cpu_temp_avg, metrics.temp.gpu_temp_avg],
      power: [metrics.cpu_power, metrics.gpu_power],
      tiers,
      cpu_residency,
      gpu_available,
      gpu_frequency: metrics.gpu_freq_mhz,
    };
    self.history.insert(
      0,
      [
        sample.value(0, RatioMode::Active),
        sample.value(0, RatioMode::Scaled),
        sample.value(1, RatioMode::Active),
        sample.value(1, RatioMode::Scaled),
      ],
    );
    self.history.truncate(60);
    self.latest = Some(sample);
  }

  fn trend(&self, index: usize, mode: RatioMode, width: usize, view: ViewType) -> Line<'static> {
    let index = index * 2 + usize::from(mode == RatioMode::Scaled);
    let values: String = if view == ViewType::Gauge {
      let value = self.history.first().and_then(|point| point[index]);
      (0..width)
        .map(|column| {
          value.map_or('·', |value| if (column as f32) < value * width as f32 { '━' } else { '─' })
        })
        .collect()
    } else {
      self
        .history
        .iter()
        .take(width.min(60))
        .rev()
        .map(|point| {
          point[index].map_or('·', |value| {
            ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][(value * 7.0).round() as usize]
          })
        })
        .collect()
    };
    Line::from(theme::value(values, if index < 2 { theme::CPU } else { theme::GPU }))
  }
}

impl Sample {
  fn value(&self, index: usize, mode: RatioMode) -> Option<f32> {
    if index == 1 && !self.gpu_available {
      return None;
    }
    match mode {
      RatioMode::Active => ratio(self.active[index]),
      RatioMode::Scaled
        if (index == 0 && self.cpu_residency) || (index == 1 && self.gpu_frequency > 0) =>
      {
        ratio(self.weighted[index])
      }
      _ => None,
    }
  }
}

impl App {
  pub(super) fn compute_overall_title(&self) -> Line<'static> {
    let Some(sample) = &self.compute.latest else {
      return Line::from(dim("CPU waiting"));
    };
    let mode = if self.soc.vm { RatioMode::Active } else { self.cfg.ratio_mode };
    let name = if mode == RatioMode::Active { "Active" } else { "Weighted" };
    Line::from(theme::value(format!("CPU {name} {}", percent(sample.value(0, mode))), theme::CPU))
  }

  pub(super) fn compute_legacy_value(
    &self,
    cluster: Option<usize>,
    mode: RatioMode,
  ) -> Option<f32> {
    let sample = self.compute.latest.as_ref()?;
    if let Some(index) = cluster {
      let tier = sample.tiers.get(index)?;
      match mode {
        RatioMode::Active => ratio(tier.active),
        RatioMode::Scaled if sample.cpu_residency => ratio(tier.weighted),
        _ => None,
      }
    } else {
      sample.value(1, mode)
    }
  }

  pub(super) fn compute_legacy_frequency(&self, cluster: Option<usize>) -> bool {
    self.compute.latest.as_ref().is_some_and(|sample| match cluster {
      Some(index) => {
        sample.cpu_residency
          && sample
            .tiers
            .get(index)
            .is_some_and(|tier| tier.cores.iter().any(|core| core.freq_mhz > 0))
      }
      None => sample.gpu_available && sample.gpu_frequency > 0,
    })
  }

  pub(super) fn compute_lines(&self, detailed: bool, width: usize) -> Vec<Line<'static>> {
    let Some(sample) = &self.compute.latest else {
      return vec![Line::from(dim("Waiting for compute sample"))];
    };
    let mode = if self.soc.vm { RatioMode::Active } else { self.cfg.ratio_mode };
    let mode_name = if mode == RatioMode::Active { "Active" } else { "Weighted" };
    let mut lines = Vec::new();
    for (index, name) in ["CPU", "GPU"].into_iter().enumerate() {
      let value = sample.value(index, mode);
      let label =
        if index == 1 && mode == RatioMode::Active { "Active (reported)" } else { mode_name };
      let mut summary = format!("{name} {label} {}", percent(value));
      if !self.soc.vm
        && let Some(temperature) = sample.temperatures[index]
          .filter(|value| value.is_finite() && *value > 0.0 && *value <= 150.0)
      {
        let with_temperature = format!("{summary} · {temperature:.1}°C");
        if with_temperature.chars().count() <= width {
          summary = with_temperature;
        }
      }
      let mut summary_lines = Vec::new();
      fields(&mut summary_lines, &[summary], width);
      lines.extend(summary_lines.into_iter().map(|line| {
        Line::from(theme::value(line.to_string(), if index == 0 { theme::CPU } else { theme::GPU }))
      }));
      if mode == RatioMode::Scaled
        && value.is_none()
        && sample.value(index, RatioMode::Active).is_some()
      {
        lines.push(Line::from(dim(format!(
          "  Reported Active {}",
          percent(sample.value(index, RatioMode::Active))
        ))));
      }
    }
    if !detailed {
      return fit_lines(lines, width);
    }
    fields(
      &mut lines,
      &[
        "Reported Active = active residency".into(),
        "Weighted = frequency-weighted residency".into(),
      ],
      width,
    );
    fields(
      &mut lines,
      &[
        format!("CPU Active {}", percent(sample.value(0, RatioMode::Active))),
        format!("Weighted {}", percent(sample.value(0, RatioMode::Scaled))),
      ],
      width,
    );
    if !sample.cpu_residency {
      lines.push(Line::from(dim("CPU frequency/weighted unavailable")));
      fields(&mut lines, &["Incomplete cores or ambiguous tick fallback".into()], width);
    }
    let reported: usize = sample.tiers.iter().map(|tier| tier.cores.len()).sum();
    let installed: usize = self.soc.cpu_tiers.iter().map(|tier| usize::from(tier.cores)).sum();
    lines.push(Line::from(text(format!("CPU cores {reported} reported / {installed} installed"))));
    let chart = if self.cfg.view_type == ViewType::Gauge {
      "gauge · current sample"
    } else {
      "history · latest 60 samples"
    };
    lines.push(Line::from(dim(format!("{mode_name} {chart}"))));
    lines.push(self.compute.trend(0, mode, width, self.cfg.view_type));
    fields(
      &mut lines,
      &[
        format!("GPU Active {}", percent(sample.value(1, RatioMode::Active))),
        format!("Weighted {}", percent(sample.value(1, RatioMode::Scaled))),
      ],
      width,
    );
    fields(
      &mut lines,
      &[
        format!("GPU mean {}", frequency(sample.gpu_frequency, sample.gpu_available)),
        format!(
          "Reported GPU cores {}",
          if !self.soc.vm && self.soc.gpu_cores > 0 {
            self.soc.gpu_cores.to_string()
          } else {
            "Unavailable".into()
          }
        ),
      ],
      width,
    );
    let supported: Vec<_> =
      self.soc.gpu_freqs.iter().skip(1).copied().filter(|value| *value > 0).collect();
    if let (Some(min), Some(max)) = (supported.iter().min(), supported.iter().max())
      && sample.gpu_available
    {
      fields(&mut lines, &[format!("GPU supported {min}–{max} MHz")], width);
    }
    if !sample.gpu_available || sample.gpu_frequency == 0 {
      lines.push(Line::from(dim("GPU residency source not established")));
      lines.push(Line::from(dim("Reported zero does not establish idle")));
    }
    for (index, name) in ["CPU", "GPU"].into_iter().enumerate() {
      let power = (!self.soc.vm)
        .then_some(sample.power[index])
        .filter(|value| value.is_finite() && *value >= 0.0);
      lines.push(Line::from(text(format!(
        "{name} reported power {}",
        power.map_or_else(|| "Unavailable".into(), |value| format!("{value:.2} W"))
      ))));
    }
    lines.push(Line::from(dim("Core/GPU MHz: active-time means")));
    lines.push(Line::from(dim("Tier aggregate includes minimum floor")));
    lines.push(Line::from(dim(format!("GPU {mode_name} {chart}"))));
    lines.push(self.compute.trend(1, mode, width, self.cfg.view_type));
    for tier in &sample.tiers {
      let label = if tier.label == "_" { "CPU".into() } else { format!("{} CPU", tier.label) };
      lines.push(Line::from(theme::value(
        format!("{label} · {} reported / {} installed", tier.cores.len(), tier.installed),
        theme::CPU,
      )));
      fields(
        &mut lines,
        &[
          format!("Active {}", percent(ratio(tier.active))),
          format!(
            "Weighted {}",
            percent(sample.cpu_residency.then(|| ratio(tier.weighted)).flatten())
          ),
          format!(
            "Tier aggregate {}",
            frequency(
              tier.frequency,
              sample.cpu_residency && tier.cores.iter().any(|core| core.freq_mhz > 0)
            )
          ),
        ],
        width,
      );
      for core in &tier.cores {
        fields(
          &mut lines,
          &[
            format!("Die {} / Core {}", core.die_id, core.core_id),
            format!("Active {}", percent(ratio(core.active_ratio))),
            format!(
              "Weighted {}",
              percent(sample.cpu_residency.then(|| ratio(core.scaled_ratio)).flatten())
            ),
            format!("Mean {}", frequency(core.freq_mhz, sample.cpu_residency)),
          ],
          width,
        );
      }
      if tier.cores.is_empty() {
        lines.push(Line::from(dim("No core measurements reported")));
      }
    }
    fit_lines(lines, width)
  }
}

#[cfg(test)]
mod tests {
  use macmon::{CpuTierInfo, CpuTierMetrics, TempMetrics};
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;

  use super::*;
  use crate::config::Config;
  use crate::tui::panels::Panel;

  fn fixture() -> (SocInfo, Metrics) {
    let info =
      |label: &str, cores| CpuTierInfo { label: label.into(), cores, freqs: vec![800, 3200] };
    let tier = |label: &str, count| CpuTierMetrics {
      label: label.into(),
      freq_mhz: 1600,
      active_ratio: 0.9,
      scaled_ratio: 0.2,
      cores: (0..count)
        .map(|id| CpuCoreMetrics {
          die_id: id,
          core_id: id,
          freq_mhz: 1600,
          active_ratio: 0.9,
          scaled_ratio: 0.2,
        })
        .collect(),
    };
    (
      SocInfo {
        cpu_tiers: vec![info("E", 2), info("P", 1)],
        gpu_cores: 18,
        gpu_freqs: vec![0, 400, 1400],
        ..Default::default()
      },
      Metrics {
        cpu_tiers: vec![tier("E", 2), tier("P", 1)],
        cpu_active_ratio: 0.37,
        cpu_scaled_ratio: 0.19,
        gpu_active_ratio: 0.23,
        gpu_scaled_ratio: 0.11,
        gpu_freq_mhz: 1000,
        temp: TempMetrics { cpu_temp_avg: Some(45.0), gpu_temp_avg: Some(40.0) },
        cpu_power: 3.5,
        gpu_power: 2.5,
        ..Default::default()
      },
    )
  }

  fn text(app: &App, detail: bool, width: usize) -> String {
    app.compute_lines(detail, width).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
  }

  fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect()
  }

  #[test]
  fn summary_uses_supplied_totals_and_current_temperature_while_details_add_real_cores() {
    let (soc, metrics) = fixture();
    let mut app = App::from_parts(soc, Config::load_from(None));
    assert!(text(&app, false, 38).contains("Waiting"));
    app.update_metrics(metrics);
    let summary = text(&app, false, 38);
    assert!(summary.contains("CPU Weighted 19.0% · 45.0°C"), "{summary}");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let compute = app.panels.targets.iter().find(|(panel, _)| *panel == Panel::Compute).unwrap().1;
    assert_eq!(terminal.backend().buffer()[(compute.x + 1, compute.y + 1)].fg, theme::CPU);
    assert!(!summary.contains("90.0%") && !summary.contains("MHz") && !summary.contains("Core"));
    let detail = text(&app, true, 76);
    for expected in [
      "CPU Active 37.0%",
      "3 reported / 3 installed",
      "Die 1 / Core 1",
      "Mean 1600 MHz",
      "GPU supported 400–1400 MHz",
      "Reported GPU cores 18",
      "3.50 W",
      "2.50 W",
    ] {
      assert!(detail.contains(expected), "missing {expected}: {detail}");
    }
    assert!(!detail.contains("Die 2 / Core 2"));
    app.cfg.ratio_mode = RatioMode::Active;
    assert!(text(&app, false, 38).contains("CPU Active 37.0%"));
    assert!(text(&app, false, 38).contains("GPU Active (reported) 23.0%"));
    let (_, mut next) = fixture();
    next.temp = TempMetrics::default();
    app.update_metrics(next);
    assert!(!text(&app, false, 38).contains("°C"));
  }

  #[test]
  fn tick_floors_partial_coverage_and_vms_never_claim_weighted_or_measured_frequency() {
    for case in [
      "tick",
      "partial",
      "missing",
      "vm",
      "duplicate",
      "metadata",
      "core-ratio",
      "tier-ratio",
      "labels",
      "metadata-zero",
      "active-no-clock",
    ] {
      let (mut soc, mut metrics) = fixture();
      match case {
        "tick" => {
          soc.cpu_tiers.truncate(1);
          metrics.cpu_tiers.truncate(1);
          for core in &mut metrics.cpu_tiers[0].cores {
            core.freq_mhz = 0;
            core.scaled_ratio = core.active_ratio;
          }
          metrics.cpu_tiers[0].freq_mhz = 800;
        }
        "partial" => {
          metrics.cpu_tiers[0].cores.pop();
        }
        "missing" => {
          metrics.cpu_tiers.clear();
        }
        "vm" => {
          soc.vm = true;
        }
        "duplicate" => {
          metrics.cpu_tiers[0].cores[1] = metrics.cpu_tiers[0].cores[0].clone();
        }
        "metadata" => {
          soc.cpu_tiers[0].freqs.clear();
        }
        "metadata-zero" => {
          soc.cpu_tiers[0].freqs = vec![0, 800];
        }
        "active-no-clock" => {
          soc.cpu_tiers.truncate(1);
          metrics.cpu_tiers.truncate(1);
          metrics.cpu_tiers[0].cores[1].freq_mhz = 0;
        }
        "core-ratio" => {
          metrics.cpu_tiers[0].cores[0].scaled_ratio = f32::NAN;
        }
        "tier-ratio" => {
          metrics.cpu_tiers[0].active_ratio = -0.1;
        }
        "labels" => {
          metrics.cpu_tiers[0].label = "S".into();
        }
        _ => unreachable!(),
      }
      let mut app = App::from_parts(soc, Config::load_from(None));
      app.update_metrics(metrics);
      let sample = app.compute.latest.as_ref().unwrap();
      assert_eq!(sample.value(0, RatioMode::Active), Some(0.37));
      assert_eq!(sample.value(0, RatioMode::Scaled), None, "{case}");
      assert!(!app.compute_legacy_frequency(Some(0)), "{case}");
      let detail = text(&app, true, 76);
      assert!(
        !detail.contains("Tier aggregate 1600 MHz") && !detail.contains("Tier aggregate 800 MHz"),
        "{case}: {detail}"
      );
      assert!(!detail.contains("Mean 1600 MHz"), "{case}: {detail}");
      if case == "partial" {
        assert!(detail.contains("2 reported / 3 installed"));
      }
      if case == "missing" {
        assert!(detail.contains("0 reported / 3 installed"));
      }
      if case == "vm" {
        assert!(detail.contains("GPU Active Unavailable"));
        assert!(detail.contains("Reported GPU cores Unavailable"));
      }
      if case != "vm" {
        assert!(text(&app, false, 38).contains("Reported Active 37.0%"));
      }
    }
  }

  #[test]
  fn complete_multi_tier_idle_retains_real_zero_and_gpu_zero_is_only_reported_activity() {
    let (soc, mut metrics) = fixture();
    metrics.cpu_active_ratio = 0.0;
    metrics.cpu_scaled_ratio = 0.0;
    for tier in &mut metrics.cpu_tiers {
      tier.active_ratio = 0.0;
      tier.scaled_ratio = 0.0;
      for core in &mut tier.cores {
        core.freq_mhz = 0;
        core.active_ratio = 0.0;
        core.scaled_ratio = 0.0;
      }
    }
    metrics.gpu_active_ratio = 0.0;
    metrics.gpu_scaled_ratio = 0.0;
    metrics.gpu_freq_mhz = 0;
    let mut app = App::from_parts(soc, Config::load_from(None));
    app.update_metrics(metrics);
    assert_eq!(app.compute.latest.as_ref().unwrap().value(0, RatioMode::Scaled), Some(0.0));
    assert_eq!(app.compute.latest.as_ref().unwrap().value(1, RatioMode::Scaled), None);
    assert!(text(&app, false, 38).contains("CPU Weighted 0.0%"));
    let detail = text(&app, true, 76);
    assert!(detail.contains("GPU Active 0.0%") && detail.contains("GPU mean Unavailable"));
    assert!(detail.contains("Reported zero does not establish idle"));
    app.soc.gpu_freqs.clear();
    let (_, next) = fixture();
    app.update_metrics(next);
    assert_eq!(app.compute.latest.as_ref().unwrap().value(1, RatioMode::Active), None);
  }

  #[test]
  fn complete_single_tier_needs_real_core_clock_and_gpu_needs_a_non_idle_table_entry() {
    let (mut soc, mut metrics) = fixture();
    soc.cpu_tiers.truncate(1);
    metrics.cpu_tiers.truncate(1);
    metrics.cpu_scaled_ratio = metrics.cpu_active_ratio;
    metrics.cpu_tiers[0].cores[1].freq_mhz = 0;
    metrics.cpu_tiers[0].cores[1].active_ratio = 0.0;
    metrics.cpu_tiers[0].cores[1].scaled_ratio = 0.0;
    let mut app = App::from_parts(soc, Config::load_from(None));
    app.update_metrics(metrics);
    assert_eq!(app.compute.latest.as_ref().unwrap().value(0, RatioMode::Scaled), Some(0.37));
    for (table, supported) in [
      (vec![], false),
      (vec![0], false),
      (vec![400], false),
      (vec![0, 0], false),
      (vec![0, 400], true),
      (vec![400, 1400], true),
    ] {
      app.soc.gpu_freqs = table;
      let (_, metrics) = fixture();
      app.update_metrics(metrics);
      assert_eq!(
        app.compute.latest.as_ref().unwrap().value(1, RatioMode::Active).is_some(),
        supported
      );
      assert!(text(&app, true, 76).contains("Reported GPU cores 18"));
      if app.soc.gpu_freqs == [400, 1400] {
        assert!(text(&app, true, 76).contains("GPU supported 1400–1400 MHz"));
      }
    }
  }

  #[test]
  fn history_is_bounded_gapped_and_switching_view_or_ratio_preserves_snapshots() {
    let (soc, metrics) = fixture();
    let mut app = App::from_parts(soc, Config::load_from(None));
    app.update_metrics(metrics);
    for _ in 0..61 {
      let (_, mut metrics) = fixture();
      metrics.cpu_tiers.clear();
      metrics.cpu_active_ratio = f32::NAN;
      metrics.gpu_freq_mhz = 0;
      app.update_metrics(metrics);
    }
    assert_eq!(app.compute.history.len(), 60);
    assert!(
      app
        .compute
        .history
        .iter()
        .all(|point| point[0].is_none() && point[1].is_none() && point[3].is_none())
    );
    let graph = app.compute.trend(0, RatioMode::Scaled, 38, ViewType::Graph).to_string();
    assert_eq!(graph, "·".repeat(38));
    let (_, metrics) = fixture();
    app.update_metrics(metrics);
    let before = app.compute.latest.as_ref().unwrap().tiers[0].cores.clone();
    app.cfg.view_type = ViewType::Gauge;
    assert!(text(&app, true, 38).contains("gauge · current sample"));
    assert!(app.compute.trend(0, RatioMode::Active, 38, ViewType::Gauge).to_string().contains('━'));
    app.cfg.ratio_mode = RatioMode::Active;
    assert_eq!(app.compute.latest.as_ref().unwrap().tiers[0].cores.len(), before.len());
  }

  #[test]
  fn full_compute_scroll_reaches_last_real_core_and_narrow_units_stay_whole() {
    let (mut soc, mut metrics) = fixture();
    soc.cpu_tiers[0].cores = 32;
    metrics.cpu_tiers[0].cores = (0..32)
      .map(|id| CpuCoreMetrics {
        die_id: 2,
        core_id: id,
        freq_mhz: u32::MAX,
        active_ratio: 0.5,
        scaled_ratio: 0.25,
      })
      .collect();
    let mut app = App::from_parts(soc, Config::load_from(None));
    app.update_metrics(metrics);
    for line in app.compute_lines(true, 26) {
      assert!(line.width() <= 26, "{}: {line}", line.width());
    }
    app.panels.focus = Panel::Compute;
    app.panels.fullscreen = true;
    let top = render(&mut app, 80, 24);
    assert!(!top.contains("Die 2 / Core 31"));
    assert!(app.handle_key(ratatui::crossterm::event::KeyCode::End.into()).is_continue());
    assert!(render(&mut app, 80, 24).contains("Die 2 / Core 31"));
    render(&mut app, 5, 3);
    assert!(render(&mut app, 80, 24).contains("Die 2 / Core 31"));
  }
}
