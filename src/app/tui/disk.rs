//! Physical disk activity and source-reported NVMe health, kept separate from logical paging.

use macmon::{DiskHealth, DiskHealthState, DiskMetric};
use ratatui::text::{Line, Span};

use super::{memory, theme};

fn clean(value: &str) -> String {
  value.chars().filter(|ch| !ch.is_control()).collect()
}

fn count(value: Option<u64>) -> String {
  value.map_or_else(|| "Unavailable".into(), |value| value.to_string())
}

fn iops(value: Option<f64>) -> String {
  value.filter(|value| value.is_finite() && *value >= 0.0).map_or_else(
    || "—".into(),
    |value| if value >= 10_000.0 { format!("{value:.1e}") } else { format!("{value:.1}") },
  )
}

fn used(value: u8) -> String {
  if value == 255 { ">254%".into() } else { format!("{value}%") }
}

fn lifetime_bytes(units: &str) -> String {
  if units.is_empty() || !units.bytes().all(|ch| ch.is_ascii_digit()) {
    return "Unavailable".into();
  }
  let Some(bytes) = units.parse::<u128>().ok().and_then(|units| units.checked_mul(512_000)) else {
    return "Unavailable".into();
  };
  let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
  let mut value = bytes as f64;
  let mut index = 0;
  while value >= 1024.0 && index < units.len() - 1 {
    value /= 1024.0;
    index += 1;
  }
  format!("{value:.2} {}", units[index])
}

fn health_label(health: &DiskHealth, sample_age_ms: u64) -> String {
  match health.state {
    DiskHealthState::Unsupported => "Unsupported".into(),
    DiskHealthState::Unavailable => "Unavailable".into(),
    DiskHealthState::Available => health.nvme.as_ref().map_or_else(
      || "Unavailable".into(),
      |nvme| {
        let age_ms = health.age_ms.map(|age| age.saturating_add(sample_age_ms));
        let age = match age_ms {
          Some(age) if age > 120_000 => "STALE · ",
          None => "Age unknown · ",
          _ => "",
        };
        let source = if nvme.critical_warning != 0 {
          "NVMe warning"
        } else if age_ms.is_some_and(|age| age <= 120_000) {
          "NVMe OK"
        } else {
          "NVMe report"
        };
        format!("{age}{source} · used {}", used(nvme.percentage_used))
      },
    ),
  }
}

fn busiest(disks: &[DiskMetric]) -> Option<&DiskMetric> {
  let activity = |disk: &DiskMetric| {
    [disk.read_bytes_per_sec, disk.write_bytes_per_sec]
      .into_iter()
      .flatten()
      .filter(|value| value.is_finite() && *value >= 0.0)
      .sum::<f64>()
  };
  disks.iter().max_by(|a, b| activity(a).total_cmp(&activity(b)).then(b.id.cmp(&a.id)))
}

/// Complete fields only: a narrow summary never cuts a number away from its unit.
pub(super) fn summary(
  disks: Option<&[DiskMetric]>,
  width: usize,
  sample_age_ms: u64,
) -> Line<'static> {
  let Some(disks) = disks else { return Line::from(theme::dim("Disk telemetry unavailable")) };
  let Some(disk) = busiest(disks) else {
    return Line::from(theme::dim("No physical disks reported"));
  };
  let name = clean(&disk.name);
  let name = if Line::from(name.clone()).width() <= width { name } else { clean(&disk.id) };
  let fields = [
    theme::text(name),
    theme::value(format!("R {}", memory::rate(disk.read_bytes_per_sec)), theme::DISK_READ),
    theme::value(format!("W {}", memory::rate(disk.write_bytes_per_sec)), theme::DISK_WRITE),
    theme::value(
      health_label(&disk.health, sample_age_ms),
      theme::disk_health(&disk.health, sample_age_ms),
    ),
    theme::text(format!("IOPS R {} W {}", iops(disk.read_iops), iops(disk.write_iops))),
  ];
  let mut spans = Vec::new();
  for field in fields {
    let separator = if spans.is_empty() { 0 } else { 3 };
    if spans.iter().map(Span::width).sum::<usize>() + separator + field.width() <= width {
      if separator > 0 {
        spans.push(theme::dim(" · "));
      }
      spans.push(field);
    }
  }
  Line::from(spans)
}

fn field(lines: &mut Vec<Line<'static>>, label: &str, value: String, width: usize) {
  let text = format!("{label}: {value}");
  let mut line = String::new();
  for word in text.split_whitespace() {
    if !line.is_empty() && Line::from(format!("{line} {word}")).width() > width {
      lines.push(Line::from(theme::text(std::mem::take(&mut line))));
    }
    if !line.is_empty() {
      line.push(' ');
    }
    for ch in word.chars() {
      if !line.is_empty() && Line::from(format!("{line}{ch}")).width() > width {
        lines.push(Line::from(theme::text(std::mem::take(&mut line))));
      }
      line.push(ch);
    }
  }
  if !line.is_empty() {
    lines.push(Line::from(theme::text(line)));
  }
}

pub(super) fn lines(
  disks: Option<&[DiskMetric]>,
  detailed: bool,
  width: usize,
  sample_age_ms: u64,
) -> Vec<Line<'static>> {
  let mut lines = vec![summary(disks, width, sample_age_ms)];
  if !detailed {
    return lines;
  }
  let mut disks: Vec<_> = disks.unwrap_or_default().iter().collect();
  disks.sort_by(|a, b| a.id.cmp(&b.id));
  for disk in disks {
    field(&mut lines, "Device", format!("{} · {}", clean(&disk.name), clean(&disk.id)), width);
    field(
      &mut lines,
      "Model",
      disk.model.as_deref().map(clean).unwrap_or_else(|| "Unavailable".into()),
      width,
    );
    field(
      &mut lines,
      "Internal",
      disk.internal.map_or_else(|| "Unavailable".into(), |value| value.to_string()),
      width,
    );
    field(&mut lines, "Capacity bytes", count(disk.size_bytes), width);
    field(&mut lines, "Read", memory::rate(disk.read_bytes_per_sec), width);
    field(&mut lines, "Write", memory::rate(disk.write_bytes_per_sec), width);
    field(
      &mut lines,
      "IOPS read / write",
      format!("{} / {}", iops(disk.read_iops), iops(disk.write_iops)),
      width,
    );
    field(&mut lines, "Driver read bytes", count(disk.read_bytes), width);
    field(&mut lines, "Driver write bytes", count(disk.write_bytes), width);
    field(&mut lines, "Driver read operations", count(disk.read_operations), width);
    field(&mut lines, "Driver write operations", count(disk.write_operations), width);
    field(&mut lines, "Health", health_label(&disk.health, sample_age_ms), width);
    field(
      &mut lines,
      "Health age",
      disk.health.age_ms.map_or_else(
        || "Unavailable".into(),
        |age| format!("{:.1} s", age.saturating_add(sample_age_ms) as f64 / 1000.0),
      ),
      width,
    );
    if disk.health.state != DiskHealthState::Available {
      continue;
    }
    let Some(nvme) = &disk.health.nvme else { continue };
    field(
      &mut lines,
      "NVMe critical-warning bits",
      format!("0x{:02X}", nvme.critical_warning),
      width,
    );
    field(
      &mut lines,
      "Endurance used",
      format!("{} (may exceed 100%; not remaining years)", used(nvme.percentage_used)),
      width,
    );
    field(
      &mut lines,
      "Available spare / threshold",
      format!(
        "{} / {}",
        nvme
          .available_spare_percent
          .map_or_else(|| "Unavailable".into(), |value| format!("{value}%")),
        nvme
          .spare_threshold_percent
          .map_or_else(|| "Unavailable".into(), |value| format!("{value}%"))
      ),
      width,
    );
    field(
      &mut lines,
      "NVMe temperature",
      nvme
        .temperature_celsius
        .filter(|value| value.is_finite())
        .map_or_else(|| "Unavailable".into(), |value| format!("{value:.1}°C")),
      width,
    );
    field(
      &mut lines,
      "Lifetime host data units",
      "1 unit = 512000 B; not NAND writes".into(),
      width,
    );
    field(&mut lines, "Lifetime host read", lifetime_bytes(&nvme.data_units_read), width);
    field(&mut lines, "Lifetime host written", lifetime_bytes(&nvme.data_units_written), width);
    for (label, value) in [
      ("Read units", &nvme.data_units_read),
      ("Written units", &nvme.data_units_written),
      ("Power cycles", &nvme.power_cycles),
      ("Power-on hours", &nvme.power_on_hours),
      ("Unsafe shutdowns", &nvme.unsafe_shutdowns),
      ("Media errors", &nvme.media_errors),
      ("Error-log entries", &nvme.error_log_entries),
    ] {
      let value = if !value.is_empty() && value.bytes().all(|ch| ch.is_ascii_digit()) {
        value.clone()
      } else {
        "Unavailable".into()
      };
      field(&mut lines, label, value, width);
    }
  }
  field(&mut lines, "Counters", "Driver totals are since driver instantiation; lifetime host units are separate from physical NAND wear.".into(), width);
  lines
}

#[cfg(test)]
mod tests {
  use std::sync::RwLock;

  use macmon::{
    MemMetrics, MemoryActivity, MemoryPressure, Metrics, NvmeHealth, TempMetrics, ThermalState,
  };
  use ratatui::Terminal;
  use ratatui::backend::TestBackend;
  use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
  use ratatui::style::Color;

  use super::*;
  use crate::config::Config;
  use crate::procs::ProcInfo;
  use crate::tui::{App, Event, Panel};

  fn disk(index: usize) -> DiskMetric {
    DiskMetric {
      id: format!("device-{index:02}"),
      name: format!("disk{index:02}"),
      read_bytes: Some(u64::MAX),
      write_bytes: Some(0),
      read_bytes_per_sec: Some(1024.0),
      write_bytes_per_sec: Some(0.0),
      read_iops: Some(1.0),
      write_iops: Some(0.0),
      ..Default::default()
    }
  }

  fn health(warning: u8, percentage_used: u8, age_ms: Option<u64>) -> DiskHealth {
    DiskHealth {
      state: DiskHealthState::Available,
      age_ms,
      nvme: Some(NvmeHealth { critical_warning: warning, percentage_used, ..Default::default() }),
    }
  }

  fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|frame| app.render(frame)).unwrap();
    term.backend().buffer().content.iter().map(|cell| cell.symbol()).collect()
  }

  fn press(app: &mut App, key: KeyCode) {
    assert!(app.handle_key(KeyEvent::new(key, KeyModifiers::NONE)).is_continue());
  }

  #[test]
  fn rates_keep_unknown_zero_and_invalid_distinct_and_fit_whole_fields() {
    assert!(summary(None, 78, 0).to_string().contains("unavailable"));
    assert!(summary(Some(&[]), 78, 0).to_string().contains("No physical disks"));
    let mut disks = [disk(0), disk(1)];
    disks[0].read_bytes_per_sec = None;
    disks[0].write_bytes_per_sec = Some(-1.0);
    disks[0].read_iops = Some(f64::NAN);
    let raw = lines(Some(&disks[..1]), true, 78, 0)
      .into_iter()
      .map(|line| line.to_string())
      .collect::<Vec<_>>()
      .join("\n");
    assert!(raw.contains("Read: Unavailable") && raw.contains("Write: Unavailable"));
    assert!(raw.contains("IOPS read / write: — / 0.0"));
    let output = summary(Some(&disks), 78, 0).to_string();
    assert!(
      output.starts_with("disk01") && output.contains("R 1.0 KiB/s") && output.contains("W 0 B/s")
    );
    for width in 0..100 {
      let line = summary(Some(&disks), width, 0);
      assert!(line.width() <= width, "{width}: {line}");
      let output = line.to_string();
      assert!(!output.contains("KiB") || output.contains("1.0 KiB/s"));
      assert!(!output.contains("W 0") || output.contains("W 0 B/s"));
    }
  }

  #[test]
  fn health_freshness_warning_and_endurance_boundaries_are_source_reported() {
    for state in [DiskHealthState::Unsupported, DiskHealthState::Unavailable] {
      assert_eq!(
        health_label(&DiskHealth { state, ..Default::default() }, 0),
        format!("{state:?}")
      );
    }
    for used_value in [0, 100, 101, 254, 255] {
      let value = health(0, used_value, Some(120_000));
      let text = health_label(&value, 0);
      assert!(text.contains(&used(used_value)) && !text.contains("STALE"));
      assert_eq!(theme::disk_health(&value, 0), Color::Green);
      assert!(health_label(&value, 1).starts_with("STALE"));
      assert!(!health_label(&value, 1).contains("NVMe OK"));
      assert_eq!(theme::disk_health(&value, 1), Color::Yellow);
    }
    assert_eq!(used(255), ">254%");
    let warning = health(0x12, 104, Some(0));
    assert!(health_label(&warning, 0).contains("NVMe warning · used 104%"));
    assert_eq!(theme::disk_health(&warning, 0), Color::Red);
    let detail = lines(Some(&[DiskMetric { health: warning, ..disk(0) }]), true, 78, 0)
      .into_iter()
      .map(|line| line.to_string())
      .collect::<Vec<_>>()
      .join("\n");
    assert!(detail.contains("NVMe critical-warning bits: 0x12"));
    assert!(health_label(&health(0, 1, None), 0).contains("Age unknown"));
    assert_eq!(theme::disk_health(&health(0, 1, None), 0), theme::DIM);
    for warning in [0, 2] {
      let mut value = disk(0);
      value.read_bytes_per_sec = Some(12.3 * 1024.0 * 1024.0);
      value.write_bytes_per_sec = Some(456.0 * 1024.0);
      value.read_iops = Some(1234.5);
      value.write_iops = Some(234.5);
      value.health = health(warning, 3, Some(1000));
      let output = summary(Some(&[value]), 78, 0).to_string();
      assert!(output.contains("R 12.3 MiB/s") && output.contains("W 456.0 KiB/s"), "{output}");
      assert!(output.contains("used 3%"), "{output}");
      assert!(output.contains(if warning == 0 { "NVMe OK" } else { "NVMe warning" }), "{output}");
    }
  }

  #[test]
  fn full_details_keep_all_128_bits_and_separate_driver_and_lifetime_host_units() {
    let mut value = disk(0);
    value.model = Some("<external>\n\x1b[31mSSD".into());
    value.health = health(0, 255, Some(60_000));
    let nvme = value.health.nvme.as_mut().unwrap();
    nvme.data_units_written = u128::MAX.to_string();
    nvme.data_units_read = "0".into();
    nvme.power_cycles = u128::MAX.to_string();
    let output = lines(Some(&[value.clone()]), true, 78, 0)
      .into_iter()
      .map(|line| line.to_string())
      .collect::<Vec<_>>()
      .join("\n");
    assert!(output.contains(&format!("Written units: {}", u128::MAX)));
    assert!(output.contains(&format!("Driver read bytes: {}", u64::MAX)));
    assert!(output.contains("1 unit = 512000 B; not NAND writes"));
    assert!(output.contains("Lifetime host written: Unavailable"));
    assert_eq!(lifetime_bytes("151000000"), "70.31 TiB");
    assert_eq!(lifetime_bytes("0"), "0.00 B");
    assert_eq!(lifetime_bytes(&u128::MAX.to_string()), "Unavailable");
    assert_eq!(lifetime_bytes("+1"), "Unavailable");
    let mut human = value.clone();
    human.health.nvme.as_mut().unwrap().data_units_written = "151000000".into();
    let human = lines(Some(&[human]), true, 78, 0)
      .into_iter()
      .map(|line| line.to_string())
      .collect::<Vec<_>>()
      .join("\n");
    assert!(human.contains("Lifetime host written: 70.31 TiB"));
    assert!(human.contains("Written units: 151000000"));
    assert!(output.contains("Endurance used: >254%") && output.contains("not remaining years"));
    assert!(!output.contains('\x1b'));
    for line in lines(Some(&[value]), true, 26, 0) {
      assert!(line.width() <= 26, "{line}");
    }
  }

  #[test]
  fn compact_80_by_24_retains_memory_network_cooling_top_five_and_disk() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let metrics = Metrics {
      memory: MemMetrics { ram_usage: 8 << 30, ram_total: 16 << 30, ..Default::default() },
      temp: TempMetrics { cpu_temp_avg: Some(45.0), gpu_temp_avg: Some(43.0) },
      ..Default::default()
    };
    assert!(
      app
        .handle_event(
          Event::Update(
            Box::new(metrics),
            Vec::new(),
            MemoryActivity {
              pressure: Some(MemoryPressure::Normal),
              swap_in_bytes_per_sec: Some(0.0),
              swap_out_bytes_per_sec: Some(1024.0),
            },
            Some(ThermalState::Fair),
            Some(vec![disk(0)]),
            std::time::Instant::now()
          ),
          &RwLock::new(1000)
        )
        .is_continue()
    );
    screen(&mut app, 80, 24);
    app.update_procs(
      app.procs_shown.showing().unwrap(),
      (0..8)
        .map(|index| ProcInfo {
          pid: 100 + index,
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
    let output = screen(&mut app, 80, 24);
    for expected in [
      "RAM 8.00 GiB / 16.00 GiB",
      "OS pressure Normal",
      "Swap in  0 B/s",
      "Swap out 1.0 KiB/s",
      "Network",
      "OS thermal Fair",
      "top 5 / 8",
      "worker7",
      "worker3",
      "7 Disk",
      "disk00",
      "R 1.0 KiB/s",
      "W 0 B/s",
      "IOPS R 1.0 W 0.0",
    ] {
      assert!(output.contains(expected), "missing {expected}: {output}");
    }
    assert!(!output.contains("worker2"));
    let showing = app.procs_shown.showing();
    press(&mut app, KeyCode::Char('7'));
    assert_eq!(app.panels.focus, Panel::Disk);
    assert!(app.cfg.disk_details);
    let expanded = screen(&mut app, 80, 24);
    assert!(expanded.contains("Swap out 1.0 KiB/s") && expanded.contains("worker3"));
    assert_eq!(app.procs_shown.showing(), showing);
  }

  #[test]
  fn disk_full_view_reaches_every_device_and_preserves_controls_during_refresh() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.disks = Some((0..40).map(disk).collect());
    screen(&mut app, 80, 24);
    press(&mut app, KeyCode::Char('7'));
    press(&mut app, KeyCode::Char('f'));
    press(&mut app, KeyCode::End);
    assert!(screen(&mut app, 80, 24).contains("device-39"));
    let offset = app.panels.fullscreen;
    assert!(
      app
        .handle_event(
          Event::Update(
            Box::default(),
            Vec::new(),
            Default::default(),
            None,
            Some((0..40).map(disk).collect()),
            std::time::Instant::now()
          ),
          &RwLock::new(1000)
        )
        .is_continue()
    );
    assert!(app.cfg.disk_details && app.panels.fullscreen == offset);
    assert!(screen(&mut app, 80, 24).contains("device-39"));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.panels.focus, Panel::Compute);
    assert!(app.cfg.disk_details && !app.panels.fullscreen);
  }

  #[test]
  fn queued_samples_keep_the_original_health_completion_time() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    let mut value = disk(0);
    value.health = health(0, 3, Some(0));
    let sampled = std::time::Instant::now() - std::time::Duration::from_secs(121);
    assert!(
      app
        .handle_event(
          Event::Update(
            Box::default(),
            Vec::new(),
            Default::default(),
            None,
            Some(vec![value]),
            sampled
          ),
          &RwLock::new(1000)
        )
        .is_continue()
    );
    assert_eq!(app.disk_sampled, Some(sampled));
    let output = screen(&mut app, 80, 24);
    assert!(output.contains("STALE") && output.contains("used 3%"), "{output}");
  }

  #[test]
  fn legacy_disk_header_and_wheel_retain_process_requests_and_route_only_disk_controls() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.cfg.panel_details = [true; 6];
    app.disks = Some(vec![disk(0)]);
    screen(&mut app, 120, 40);
    let showing = app.procs_shown.showing();
    let rect = app.panels.targets.iter().find(|(panel, _)| *panel == Panel::Disk).unwrap().1;
    app.handle_mouse(MouseEvent {
      kind: MouseEventKind::Down(MouseButton::Left),
      column: rect.x + 2,
      row: rect.y,
      modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.panels.focus, Panel::Disk);
    assert!(app.cfg.disk_details);
    assert_eq!(app.procs_shown.showing(), showing);
    app.handle_mouse(MouseEvent {
      kind: MouseEventKind::ScrollDown,
      column: rect.x + 2,
      row: rect.y + 1,
      modifiers: KeyModifiers::NONE,
    });
    let output = screen(&mut app, 120, 40);
    assert!(output.contains("7 Disk") && output.contains("Device: disk00"));
    press(&mut app, KeyCode::Char('f'));
    assert!(screen(&mut app, 120, 40).contains("Device: disk00"));
    assert_eq!(app.procs_shown.showing(), showing);
  }

  #[test]
  fn legacy_disk_expands_into_spare_rows_and_restores_the_summary_without_changing_requests() {
    let mut app = App::from_parts(Default::default(), Config::load_from(None));
    app.cfg.panel_details = [true; 6];
    app.disks = Some(vec![disk(0)]);
    screen(&mut app, 200, 50);
    let showing = app.procs_shown.showing();
    let (original, fans, summary) = app.legacy_layout(ratatui::layout::Rect::new(0, 0, 200, 50));
    assert_eq!(summary.unwrap().height, 3);
    press(&mut app, KeyCode::Char('7'));
    let expanded = screen(&mut app, 200, 50);
    let (plan, expanded_fans, detail) =
      app.legacy_layout(ratatui::layout::Rect::new(0, 0, 200, 50));
    assert_eq!(detail.unwrap().height, 12);
    assert_eq!(plan.top, original.top);
    assert_eq!(fans, expanded_fans);
    assert!(plan.proc.unwrap().height >= 6);
    assert!(expanded.contains("devices · detail") && expanded.contains("Device: disk00"));
    assert_eq!(app.procs_shown.showing(), showing);
    press(&mut app, KeyCode::Char('7'));
    screen(&mut app, 200, 50);
    assert_eq!(app.legacy_layout(ratatui::layout::Rect::new(0, 0, 200, 50)).2, summary);
    assert_eq!(app.cfg.panel_details, [true; 6]);
    assert_eq!(app.procs_shown.showing(), showing);
  }
}
