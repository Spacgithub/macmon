#[cfg(test)]
mod fan_preview {
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::time::Instant;

  use super::super::fan_protocol::Capabilities;
  use super::*;

  const SAMPLE_MSEC: u32 = 5000;
  const SYNTHETIC_FAULT: &str = "synthetic fan fixture sampling failure";

  fn preview_metrics(mode: &str) -> Metrics {
    let mut metrics = test_metrics();
    metrics.network = network_app().network;
    metrics.fans = match mode {
      "missing" => Vec::new(),
      "one" => vec![FanMetric { name: "fan0".into(), rpm: 1800, max_rpm: None }],
      "zero" => {
        ["fan0", "fan1"].map(|name| FanMetric { name: name.into(), rpm: 0, max_rpm: None }).into()
      }
      _ => vec![
        FanMetric { name: "fan0".into(), rpm: 1800, max_rpm: None },
        FanMetric { name: "fan1".into(), rpm: 2400, max_rpm: None },
      ],
    };
    metrics
  }

  fn preview_app(mode: &str, animate: bool) -> App {
    let config = serde_json::from_value(serde_json::json!({
      "interval": SAMPLE_MSEC,
      "animate_fans": animate,
    }))
    .unwrap();
    let mut soc = test_soc();
    soc.chip_name = "SYNTHETIC FAN FIXTURE".into();
    let mut app = App::from_parts(soc, config);
    app.update_metrics(preview_metrics(mode));
    app
  }

  fn graphical_app(mode: &str, animate: bool) -> App {
    let mut app = preview_app(mode, animate);
    app.capabilities = Some(Capabilities {
      foreground: [230, 230, 230],
      background: [0, 0, 0],
      accent: Some([0, 200, 200]),
      cell_pixels: (9, 18),
    });
    app
  }

  fn assert_saved_intent(app: &App, interval: &RwLock<u32>, animate: bool) {
    assert_eq!(app.cfg.interval(), SAMPLE_MSEC);
    assert_eq!(*interval.read().unwrap(), SAMPLE_MSEC);
    let saved = serde_json::to_value(&app.cfg).unwrap();
    assert_eq!(saved["interval"], SAMPLE_MSEC);
    assert_eq!(saved["animate_fans"], animate);
  }

  #[test]
  fn graphical_fan_cards_keep_actual_names_rpm_and_full_width_network() {
    for (mode, count, active) in [("one", 1, true), ("two", 2, true), ("zero", 2, false)] {
      let mut app = graphical_app(mode, true);
      let network_before = serde_json::to_value(&app.network).unwrap();
      let buffer = render_buffer(&mut app, 120, 40);
      let screen = screen_text(&buffer);
      assert_eq!(app.placements.len(), count, "{mode}");
      assert_eq!(screen.matches("Cooling").count(), count, "{mode}: {screen}");
      for fan in app.cooling.items() {
        assert!(screen.contains(&fan.name));
        assert!(screen.contains(&format!("{} RPM", fan.rpm)));
      }
      assert_eq!(app.fan_frames_active(), active, "{mode}");
      let network = app.layout(buffer.area).network.unwrap();
      assert_eq!(network.width, 118);
      assert!(row(&buffer, network.y).contains("Network 3/4"));
      assert!(row(&buffer, network.y + 2).contains("en0"));
      assert!(row(&buffer, network.y + 2).contains("1.0 MiB/s"));
      assert_eq!(serde_json::to_value(&app.network).unwrap(), network_before);
      for place in &app.placements {
        assert!((180..=240).contains(&place.side));
        assert_eq!(place.card.intersection(place.image), place.image);
        assert!(place.card.intersection(network).is_empty());
      }
      assert_saved_intent(&app, &RwLock::new(SAMPLE_MSEC), true);
    }
  }

  #[test]
  fn graphical_fan_gates_preserve_manual_intent_and_saved_five_second_sampling() {
    let mut app = graphical_app("two", true);
    let interval = RwLock::new(SAMPLE_MSEC);
    render_buffer(&mut app, 120, 40);
    assert!(app.fan_frames_active());
    for event in [Event::FocusLost, Event::FocusGained] {
      let focused = matches!(event, Event::FocusGained);
      let _ = app.apply_event(event, &interval).unwrap();
      assert_eq!(app.fan_frames_active(), focused);
      assert_saved_intent(&app, &interval, true);
    }
    let _ = app.apply_event(Event::Key(key('?')), &interval).unwrap();
    render_buffer(&mut app, 120, 40);
    assert!(app.help.is_some() && !app.fan_frames_active());
    let _ = app.apply_event(Event::Key(key('a')), &interval).unwrap();
    assert_saved_intent(&app, &interval, true);
    let _ = app
      .apply_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)), &interval)
      .unwrap();
    render_buffer(&mut app, 120, 40);
    assert!(app.fan_frames_active());
    let _ = app.apply_event(Event::Key(key('a')), &interval).unwrap();
    assert!(!app.fan_frames_active());
    for event in
      [Event::FocusLost, Event::FocusGained, Event::Update(Box::new(preview_metrics("two")))]
    {
      let _ = app.apply_event(event, &interval).unwrap();
      render_buffer(&mut app, 120, 40);
      assert!(!app.fan_frames_active());
      assert_saved_intent(&app, &interval, false);
    }
    let _ = app.apply_event(Event::Key(key('a')), &interval).unwrap();
    render_buffer(&mut app, 60, 18);
    assert!(!app.fan_frames_active() && app.placements.is_empty());
    assert_saved_intent(&app, &interval, true);
    render_buffer(&mut app, 120, 40);
    assert!(app.fan_frames_active());
    assert_saved_intent(&app, &interval, true);
  }

  #[test]
  fn graphical_fan_update_events_keep_identity_and_stop_zero_or_missing_readings() {
    let mut app = graphical_app("two", true);
    let interval = RwLock::new(SAMPLE_MSEC);
    app.cooling.frame(Duration::from_millis(100));
    let angles: Vec<_> = app.cooling.items().iter().map(|fan| fan.rotor.angle).collect();
    assert!(angles[1] > angles[0] && angles[0] > 0.0);
    let mut metrics = preview_metrics("two");
    metrics.fans.reverse();
    let _ = app.apply_event(Event::Update(Box::new(metrics)), &interval).unwrap();
    assert_eq!(app.cooling.items()[0].name, "fan1");
    assert_eq!(app.cooling.items()[0].rotor.angle, angles[1]);
    assert_eq!(app.cooling.items()[1].rotor.angle, angles[0]);
    let _ = app.apply_event(Event::Update(Box::new(preview_metrics("zero"))), &interval).unwrap();
    render_buffer(&mut app, 120, 40);
    assert!(!app.fan_frames_active());
    assert!(app.cooling.items().iter().all(|fan| fan.rpm == 0 && fan.rotor.velocity == 0.0));
    let stopped: Vec<_> = app.cooling.items().iter().map(|fan| fan.rotor.angle).collect();
    app.cooling.frame(Duration::from_secs(1));
    assert_eq!(app.cooling.items().iter().map(|fan| fan.rotor.angle).collect::<Vec<_>>(), stopped);
    let _ = app.apply_event(Event::Update(Box::new(preview_metrics("missing"))), &interval).unwrap();
    let screen = render_to_string(&mut app, 120, 40);
    assert!(app.cooling.items().is_empty() && app.placements.is_empty());
    assert!(!app.fan_frames_active() && !screen.contains("Cooling"));
    assert!(screen.contains("Network 3/4"));
    assert_saved_intent(&app, &interval, true);
  }

  fn preview_report(path: &std::path::Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
  }

  /// Opt-in synthetic terminal fixture; never samples hardware or saves the user's settings.
  /// --nocapture --test-threads=1. All readings are synthetic; no real sampler runs here.
  #[test]
  #[ignore = "synthetic graphical fixture needs a private PTY and external frame/cleanup checks"]
  fn native_graphical_fan_fixture() {
    assert!(unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }, "requires a private PTY");
    let mode = std::env::var("MACMON_FAN_PREVIEW_MODE").unwrap_or_else(|_| "two".into());
    assert!(["one", "two", "zero", "missing"].contains(&mode.as_str()));
    let animation = std::env::var("MACMON_FAN_PREVIEW_ANIMATION").unwrap_or_else(|_| "on".into());
    assert!(["on", "off"].contains(&animation.as_str()));
    let animate = animation == "on";
    let exit = std::env::var("MACMON_FAN_PREVIEW_EXIT").unwrap_or_else(|_| "quit".into());
    assert!(["quit", "error", "panic"].contains(&exit.as_str()));
    let fault_delay = std::env::var("MACMON_FAN_PREVIEW_FAULT_MSEC")
      .ok().and_then(|value| value.parse::<u64>().ok()).unwrap_or(1000).clamp(1, 2000);
    let minimum = if exit == "quit" { 1 } else { 2 };
    let seconds = std::env::var("MACMON_FAN_PREVIEW_SECONDS")
      .ok()
      .and_then(|value| value.parse::<u64>().ok())
      .unwrap_or(20)
      .clamp(minimum, 60);
    let path = std::env::var_os("MACMON_FAN_PREVIEW_REPORT")
      .map(std::path::PathBuf::from)
      .unwrap_or_else(|| {
        std::path::PathBuf::from(format!(
          "/private/tmp/macmon-fan-preview-{}-{mode}-{animate}-{exit}.json",
          std::process::id()
        ))
      });
    assert!(path.starts_with("/private/tmp") || path.starts_with("/tmp"));
    let mut app = preview_app(&mode, animate);
    let mut report = serde_json::json!({
      "synthetic": true,
      "hardware_sampling": false,
      "chip_name": app.soc.chip_name,
      "fans": preview_metrics(&mode).fans,
      "mode": mode,
      "exit_mode": exit,
      "animation_initially_enabled": animate,
      "sample_interval_msec": SAMPLE_MSEC,
      "target_submission_fps": 60,
      "measured_display_fps": null,
      "frame_measurement": "count image commands in the external private PTY",
      "seconds_limit": seconds,
      "phase": "starting"
    });
    preview_report(&path, &report);
    let interval = Arc::new(RwLock::new(SAMPLE_MSEC));
    let stop = Arc::new(AtomicBool::new(false));
    let updates = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    let producer = {
      let (tx, stop, updates, interval) =
        (tx.clone(), stop.clone(), updates.clone(), interval.clone());
      let (mode, exit, path, base) = (mode.clone(), exit.clone(), path.clone(), report.clone());
      std::thread::spawn(move || {
        let mut active_since = None;
        let mut next_sample = started + Duration::from_millis(u64::from(SAMPLE_MSEC));
        loop {
          if stop.load(Ordering::SeqCst) {
            break;
          }
          let now = Instant::now();
          if super::super::TERM_ACTIVE.load(Ordering::SeqCst) {
            active_since.get_or_insert(now);
          }
          if active_since.is_some_and(|active| now.duration_since(active) >= Duration::from_millis(fault_delay))
          {
            if exit == "error" {
              let _ = tx.send(Event::Failed(SYNTHETIC_FAULT.into()));
              break;
            }
            if exit == "panic" {
              let production_hook = std::panic::take_hook();
              std::panic::set_hook(Box::new(move |info| {
                production_hook(info);
                let mut report = base.clone();
                report["phase"] = "production_panic_hook_returned_before_abort".into();
                report["terminal_active_after_hook"] =
                  super::super::TERM_ACTIVE.load(Ordering::SeqCst).into();
                report["sample_updates_sent"] = updates.load(Ordering::SeqCst).into();
                report["sample_interval_after"] = (*interval.read().unwrap()).into();
                let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap());
                std::process::abort();
              }));
              panic!("synthetic fan fixture panic");
            }
          }
          if now.duration_since(started) >= Duration::from_secs(seconds) {
            let _ = tx.send(Event::Key(key('q')));
            // The normal q exits first; Ctrl-C bounds the run if an overlay consumed it.
            let _ = tx.send(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
            break;
          }
          if now >= next_sample {
            if tx.send(Event::Update(Box::new(preview_metrics(&mode)))).is_err() {
              break;
            }
            updates.fetch_add(1, Ordering::SeqCst);
            next_sample += Duration::from_millis(u64::from(SAMPLE_MSEC));
          }
          std::thread::sleep(Duration::from_millis(10));
        }
      })
    };
    super::super::fan_images::FRAME_SUBMISSIONS.store(0, Ordering::Relaxed);
    let result = app.run_terminal(tx, rx, interval.as_ref());
    stop.store(true, Ordering::SeqCst);
    producer.join().unwrap();
    report["phase"] = "returned".into();
    report["elapsed_seconds"] = started.elapsed().as_secs_f64().into();
    report["submitted_image_frames"] =
      super::super::fan_images::FRAME_SUBMISSIONS.load(Ordering::Relaxed).into();
    report["native_images_enabled"] = app.capabilities.is_some().into();
    report["result"] = match &result {
      Ok(()) => "ok".into(),
      Err(error) => error.to_string().into(),
    };
    report["sample_updates_sent"] = updates.load(Ordering::SeqCst).into();
    report["sample_interval_after"] = (*interval.read().unwrap()).into();
    report["saved_config_after"] = serde_json::to_value(&app.cfg).unwrap();
    report["terminal_active_after"] = super::super::TERM_ACTIVE.load(Ordering::SeqCst).into();
    preview_report(&path, &report);
    assert_saved_intent(&app, interval.as_ref(), app.cfg.animate_fans);
    if exit == "error" {
      assert_eq!(result.unwrap_err().to_string(), SYNTHETIC_FAULT);
    } else {
      result.unwrap();
    }
    assert!(!super::super::TERM_ACTIVE.load(Ordering::SeqCst));
  }
}
