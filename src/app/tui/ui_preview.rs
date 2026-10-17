// Native buffer previews and a bounded draw benchmark, explicitly opt-in and test-only.
#[cfg(test)]
mod ui_preview {
  use std::hint::black_box;
  use std::path::Path;
  use std::time::Instant;

  use super::*;

  /// Root agent's private PTY drives real hardware, resizing and keys without reading/saving settings.
  #[test]
  #[ignore = "interactive real-hardware terminal check"]
  fn real_hardware_detail_fixture() {
    let mut app = App::from_parts(SocInfo::new().unwrap(), Config::load_from(None));
    app.run_loop(None).unwrap();
  }

  fn demo() -> App {
    let mut app = network_app();
    if std::env::var("MACMON_UI_PREVIEW_VIEW").as_deref() == Ok("gauge") {
      app.cfg.view_type = ViewType::Gauge;
    }
    for step in 0..120 {
      let t = step as f32;
      let mut metrics = test_metrics();
      metrics.cpu_tiers[0].scaled_ratio = 0.18 + (t * 0.11).sin().abs() * 0.36;
      metrics.cpu_tiers[1].scaled_ratio = 0.25 + (t * 0.09).cos().abs() * 0.45;
      metrics.gpu_scaled_ratio = 0.12 + (t * 0.07).sin().abs() * 0.31;
      metrics.memory.ram_usage = ((18.0 + (t * 0.08).sin()) * (1u64 << 30) as f32) as u64;
      metrics.cpu_power = 3.5 + (t * 0.10).sin().abs() * 6.0;
      metrics.gpu_power = 1.0 + (t * 0.13).cos().abs() * 3.0;
      metrics.ane_power = 0.1 + (t * 0.12).sin().abs() * 0.4;
      metrics.sys_power = 10.0 + metrics.cpu_power + metrics.gpu_power;
      metrics.network = Some(vec![
        NetworkMetric {
          name: "en0".into(),
          rx_bytes: 28 << 30,
          tx_bytes: 5 << 30,
          rx_bytes_per_sec: Some(4_521_984.0),
          tx_bytes_per_sec: Some(430_080.0),
        },
        NetworkMetric {
          name: "utun0".into(),
          rx_bytes: 6 << 30,
          tx_bytes: 2 << 30,
          rx_bytes_per_sec: Some(731_136.0),
          tx_bytes_per_sec: Some(91_136.0),
        },
        NetworkMetric {
          name: "awdl0".into(),
          rx_bytes: 12 << 20,
          tx_bytes: 8 << 20,
          rx_bytes_per_sec: Some(0.0),
          tx_bytes_per_sec: Some(0.0),
        },
      ]);
      app.update_metrics(metrics);
    }
    let names = [
      "WindowServer",
      "Safari",
      "macmon",
      "Terminal",
      "kernel_task",
      "Code",
      "Finder",
      "Spotlight",
      "Music",
      "Preview",
      "Notes",
      "Messages",
    ];
    let procs = names
      .into_iter()
      .enumerate()
      .map(|(i, name)| {
        let mut proc = test_procs()[1].clone();
        proc.pid = 1000 + i as i32;
        proc.name = name.into();
        proc.user = if name == "kernel_task" { "root" } else { "user" }.into();
        proc.cpu_pct = 19.0 / (i + 1) as f32;
        proc.mem_bytes = (1400 / (i + 1) as u64) << 20;
        proc.gpu_pct = if i == 0 { 12.5 } else { 0.0 };
        proc.power_w = Some(1.2 / (i + 1) as f32);
        proc
      })
      .collect();
    with_procs(app, procs)
  }

  #[test]
  #[ignore = "opt-in PTY fixture with synthetic 3000 RPM; no hardware sampling"]
  fn live_fan_fixture() {
    let duration: u64 = std::env::var("MACMON_UI_FIXTURE_SECONDS").unwrap().parse().unwrap();
    assert!((1..=300).contains(&duration));
    let report = std::env::var("MACMON_UI_FIXTURE_REPORT").unwrap();
    let mut app = demo();
    app.cfg.animate_fans = std::env::var("MACMON_UI_FIXTURE_MOTION").as_deref() == Ok("on");
    // These are fixture values, never a fallback for real hardware readings.
    let fixture = || Metrics {
      fans: vec![FanMetric { name: "fixture fan".into(), rpm: 3000, max_rpm: None }],
      network: Some(vec![NetworkMetric {
        name: "en0".into(), rx_bytes: 28 << 30, tx_bytes: 5 << 30,
        rx_bytes_per_sec: Some(4_521_984.0), tx_bytes_per_sec: Some(430_080.0),
      }]),
      ..test_metrics()
    };
    app.update_metrics(fixture());
    let interval = RwLock::new(1000);
    let (tx, rx) = mpsc::channel();
    let (mut terminal, guard) = super::super::enter_term().unwrap();
    super::super::run_inputs_thread(tx, 250, None);
    let start = Instant::now();
    let mut next_sample = start + Duration::from_secs(1);
    let (mut redraw, mut frames, mut ticks, mut fan_frames, mut samples) = (true, 0, 0, 0, 0);
    while start.elapsed().as_secs() < duration {
      if redraw {
        terminal.draw(|f| app.render(f)).unwrap();
        frames += 1;
      }
      if Instant::now() >= next_sample {
        app.update_metrics(fixture());
        if let Some(showing) = app.procs_shown.showing() {
          app.update_procs(showing, test_procs());
        }
        next_sample += Duration::from_secs(1);
        samples += 1;
        redraw = true;
        continue;
      }
      match rx.recv_timeout(next_sample.saturating_duration_since(Instant::now())) {
        Ok(event) => {
          let tick = matches!(event, Event::Tick);
          ticks += u32::from(tick);
          match app.apply_event(event, &interval).unwrap() {
            ControlFlow::Continue(changed) => {
              redraw = changed;
              fan_frames += u32::from(tick && changed);
            }
            ControlFlow::Break(()) => break,
          }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => redraw = false,
        Err(err) => panic!("fixture inputs: {err}"),
      }
    }
    drop(guard);
    std::fs::write(report, serde_json::to_vec_pretty(&serde_json::json!({
      "synthetic_rpm": 3000, "hardware_sampling": false, "seconds": start.elapsed().as_secs_f64(),
      "motion": app.cfg.animate_fans, "frames": frames, "ticks": ticks,
      "fan_frames": fan_frames, "samples": samples, "interval_ms": app.cfg.interval(),
    })).unwrap()).unwrap();
  }

  #[test]
  #[ignore = "writes fixture previews only when explicitly requested"]
  fn export_buffers() {
    let output = std::env::var("MACMON_UI_PREVIEW_OUT").expect("set MACMON_UI_PREVIEW_OUT");
    std::fs::create_dir_all(&output).unwrap();
    for (width, height) in [(80, 24), (120, 40), (60, 18)] {
      let mut app = demo();
      let buf = render_buffer(&mut app, width, height);
      let header_y = app.layout(buf.area).network.unwrap().y + 1;
      let network_header = row(&buf, header_y);
      match std::env::var("MACMON_UI_PREVIEW_KIND").as_deref() {
        Ok("baseline") => assert!(!network_header.contains("↓ RX/s")),
        Ok("new") => assert!(network_header.contains("↓ RX/s")),
        _ => panic!("set MACMON_UI_PREVIEW_KIND to baseline or new"),
      }
      let cells: Vec<_> = buf
        .content
        .iter()
        .map(|cell| {
          serde_json::json!({
            "ch": cell.symbol(), "fg": format!("{:?}", cell.fg),
            "bg": format!("{:?}", cell.bg),
            "bold": cell.modifier.contains(Modifier::BOLD),
            "dim": cell.modifier.contains(Modifier::DIM),
            "reverse": cell.modifier.contains(Modifier::REVERSED)
          })
        })
        .collect();
      let snapshot = serde_json::json!({"width": width, "height": height, "cells": cells});
      let path = Path::new(&output).join(format!("{width}x{height}.json"));
      std::fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    }
  }

  #[test]
  #[ignore = "bounded render microbenchmark, not a hardware power measurement"]
  fn draw_benchmark() {
    let output = std::env::var("MACMON_UI_PREVIEW_OUT").expect("set MACMON_UI_PREVIEW_OUT");
    std::fs::create_dir_all(&output).unwrap();
    let mut results = vec![];
    for (width, height) in [(80, 24), (120, 40), (200, 50)] {
      let mut app = demo();
      let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
      for _ in 0..200 {
        terminal.draw(|frame| app.render(frame)).unwrap();
      }
      let mut runs = vec![];
      for _ in 0..9 {
        let start = Instant::now();
        for _ in 0..1000 {
          black_box(terminal.draw(|frame| app.render(frame)).unwrap());
        }
        runs.push(start.elapsed().as_secs_f64() * 1_000.0);
      }
      runs.sort_by(f64::total_cmp);
      results.push(serde_json::json!({"width": width, "height": height, "median_us_per_frame": runs[4], "min_us_per_frame": runs[0], "max_us_per_frame": runs[8]}));
    }
    std::fs::write(
      Path::new(&output).join("draw-cost.json"),
      serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string(&results).unwrap());
  }
}
