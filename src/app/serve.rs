//! HTTP serving and launchd support.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use macmon::{Metrics, SocInfo};

use crate::metrics_to_json_value;

pub type SharedMetrics = Arc<RwLock<Option<Sample>>>;

pub struct Sample {
  metrics: Metrics,
  sequence: u64,
  sampled_at: String,
  sampled: Instant,
  interval_ms: u32,
}

pub fn publish_sample(shared: &SharedMetrics, metrics: Metrics, interval_ms: u32) {
  let sampled = Instant::now();
  let sampled_at = chrono::Utc::now().to_rfc3339();
  let mut latest = shared.write().unwrap();
  let sequence = latest.as_ref().map_or(1, |sample| sample.sequence.saturating_add(1));
  *latest = Some(Sample { metrics, sequence, sampled_at, sampled, interval_ms });
}

fn escape_label_value(value: &str) -> String {
  value.replace('\\', r"\\").replace('\n', r"\n").replace('"', r#"\""#)
}

#[rustfmt::skip]
#[allow(deprecated)] // keeps exporting macmon_ecpu_* / macmon_pcpu_*
fn to_prometheus(m: &Metrics, soc: &SocInfo) -> String {
  let chip = escape_label_value(&soc.chip_name);
  let l = format!(r#"chip="{chip}""#);

  macro_rules! metric_name {
    ($name:literal) => {
      concat!("macmon_", $name)
    };
  }

  macro_rules! gauge_head {
    ($out:expr, $name:literal, $help:literal) => {
      let name = metric_name!($name);
      $out.push_str(&format!("# HELP {} {}\n# TYPE {} gauge\n", name, $help, name));
    };
  }

  // `gauge_any!`: a gauge every Mac and VM has.
  macro_rules! gauge_any {
    ($out:expr, $name:literal, $help:literal, $value:expr) => {
      gauge_head!($out, $name, $help);
      let name = metric_name!($name);
      $out.push_str(&format!("{}{{{l}}} {}\n\n", name, $value));
    };
  }

  // `tiers_any!`: a per-tier gauge (`tier="E"` etc.) every Mac and VM has; none without tiers.
  macro_rules! tiers_any {
    ($out:expr, $name:literal, $help:literal, $field:ident) => {
      if !m.cpu_tiers.is_empty() {
        gauge_head!($out, $name, $help);
        let name = metric_name!($name);
        for tier in &m.cpu_tiers {
          let tier_label = escape_label_value(&tier.label);
          $out.push_str(&format!("{name}{{{l},tier=\"{tier_label}\"}} {}\n", tier.$field));
        }
        $out.push('\n');
      }
    };
  }

  // `gauge_mac!`: a gauge only a real Mac has (power counters, frequency tables); left out in a VM.
  macro_rules! gauge_mac {
    ($($args:tt)*) => {
      if !soc.vm {
        gauge_any!($($args)*);
      }
    };
  }
  // `tiers_mac!`: a per-tier gauge only a real Mac has (frequency tables); left out in a VM.
  macro_rules! tiers_mac {
    ($($args:tt)*) => {
      if !soc.vm {
        tiers_any!($($args)*);
      }
    };
  }

  let mut out = String::new();
  if let Some(value) = m.temp.cpu_temp_avg {
    gauge_any!(out, "cpu_temp_celsius", "Average CPU temperature in Celsius", value);
  }
  if let Some(value) = m.temp.gpu_temp_avg {
    gauge_any!(out, "gpu_temp_celsius", "Average GPU temperature in Celsius", value);
  }
  gauge_any!(out, "memory_ram_total_bytes", "Total RAM in bytes", m.memory.ram_total);
  gauge_any!(out, "memory_ram_used_bytes", "Used RAM in bytes", m.memory.ram_usage);
  gauge_any!(out, "memory_swap_total_bytes", "Total swap in bytes", m.memory.swap_total);
  gauge_any!(out, "memory_swap_used_bytes", "Used swap in bytes", m.memory.swap_usage);
  gauge_any!(out, "cpu_scaled_ratio", "Combined frequency-scaled CPU ratio (0–1), weighted by core count", m.cpu_scaled_ratio);
  gauge_any!(out, "cpu_usage_ratio", "DEPRECATED: use macmon_cpu_scaled_ratio", m.cpu_scaled_ratio);
  gauge_any!(out, "cpu_active_ratio", "Combined CPU active residency ratio (not frequency-scaled, 0–1), weighted by core count", m.cpu_active_ratio);
  gauge_mac!(out, "ecpu_freq_mhz", "DEPRECATED: use macmon_cpu_tier_freq_mhz. Lowest CPU tier frequency in MHz", m.ecpu_freq_mhz);
  gauge_any!(out, "ecpu_scaled_ratio", "DEPRECATED: use macmon_cpu_tier_scaled_ratio. Lowest CPU tier frequency-scaled ratio (0–1)", m.ecpu_scaled_ratio);
  gauge_any!(out, "ecpu_usage_ratio", "DEPRECATED: use macmon_cpu_tier_scaled_ratio", m.ecpu_scaled_ratio);
  gauge_any!(out, "ecpu_active_ratio", "DEPRECATED: use macmon_cpu_tier_active_ratio. Lowest CPU tier active residency ratio (not frequency-scaled, 0–1)", m.ecpu_active_ratio);
  gauge_mac!(out, "pcpu_freq_mhz", "DEPRECATED: use macmon_cpu_tier_freq_mhz. Highest CPU tier frequency in MHz", m.pcpu_freq_mhz);
  gauge_any!(out, "pcpu_scaled_ratio", "DEPRECATED: use macmon_cpu_tier_scaled_ratio. Highest CPU tier frequency-scaled ratio (0–1)", m.pcpu_scaled_ratio);
  gauge_any!(out, "pcpu_usage_ratio", "DEPRECATED: use macmon_cpu_tier_scaled_ratio", m.pcpu_scaled_ratio);
  gauge_any!(out, "pcpu_active_ratio", "DEPRECATED: use macmon_cpu_tier_active_ratio. Highest CPU tier active residency ratio (not frequency-scaled, 0–1)", m.pcpu_active_ratio);
  tiers_mac!(out, "cpu_tier_freq_mhz", "CPU tier frequency in MHz", freq_mhz);
  tiers_any!(out, "cpu_tier_scaled_ratio", "CPU tier frequency-scaled ratio (0–1)", scaled_ratio);
  tiers_any!(out, "cpu_tier_active_ratio", "CPU tier active residency ratio (not frequency-scaled, 0–1)", active_ratio);
  gauge_mac!(out, "gpu_freq_mhz", "GPU frequency in MHz", m.gpu_freq_mhz);
  gauge_any!(out, "gpu_scaled_ratio", "GPU frequency-scaled ratio (0–1)", m.gpu_scaled_ratio);
  gauge_any!(out, "gpu_usage_ratio", "DEPRECATED: use macmon_gpu_scaled_ratio", m.gpu_scaled_ratio);
  gauge_any!(out, "gpu_active_ratio", "GPU active residency ratio (not frequency-scaled, 0–1)", m.gpu_active_ratio);
  gauge_mac!(out, "cpu_power_watts", "CPU power consumption in Watts", m.cpu_power);
  gauge_mac!(out, "gpu_power_watts", "GPU power consumption in Watts", m.gpu_power);
  gauge_mac!(out, "ane_power_watts", "Apple Neural Engine power consumption in Watts", m.ane_power);
  gauge_mac!(out, "all_power_watts", "Combined CPU+GPU+ANE power consumption in Watts", m.all_power);
  gauge_mac!(out, "sys_power_watts", "Total system power consumption in Watts", m.sys_power);
  gauge_mac!(out, "ram_power_watts", "RAM power consumption in Watts", m.ram_power);
  gauge_mac!(out, "gpu_ram_power_watts", "GPU RAM power consumption in Watts", m.gpu_ram_power);
  if !m.fans.is_empty() {
    gauge_head!(out, "fan_speed_rpm", "Fan speed in revolutions per minute");
    let fan_speed_rpm = metric_name!("fan_speed_rpm");
    for fan in &m.fans {
      let fan_name = escape_label_value(&fan.name);
      out.push_str(&format!("{fan_speed_rpm}{{{l},fan=\"{fan_name}\"}} {}\n", fan.rpm));
    }
    out.push('\n');
  }
  if let Some(network) = &m.network {
    for (transmit, direction) in [(false, "receive"), (true, "transmit")] {
      for (rates, kind, suffix) in [(false, "counter", "bytes_total"), (true, "gauge", "bytes_per_second")] {
        let name = format!("macmon_network_{direction}_{suffix}");
        let help = if rates { "Bytes per second" } else { "Cumulative bytes, reset when the interface restarts" };
        out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
        for metric in network {
          let interface = escape_label_value(&metric.name);
          let bytes = if transmit { metric.tx_bytes } else { metric.rx_bytes };
          let rate = if transmit { metric.tx_bytes_per_sec } else { metric.rx_bytes_per_sec };
          let value = if rates { rate.map(|x| x.to_string()) } else { Some(bytes.to_string()) };
          if let Some(value) = value {
            out.push_str(&format!("{name}{{{l},interface=\"{interface}\"}} {value}\n"));
          }
        }
        out.push('\n');
      }
    }
  }
  out
}

fn json_value(m: &Metrics, soc: &SocInfo, timestamp: &str) -> serde_json::Value {
  let mut doc = metrics_to_json_value(m).unwrap_or_default();
  doc["soc"] = serde_json::to_value(soc).unwrap_or_default();
  doc["timestamp"] = timestamp.into();
  doc
}

fn to_json(m: &Metrics, soc: &SocInfo) -> String {
  json_value(m, soc, &chrono::Utc::now().to_rfc3339()).to_string()
}

fn to_sample(sample: &Sample, soc: &SocInfo) -> String {
  serde_json::json!({
    "sequence": sample.sequence,
    "sampled_at": sample.sampled_at,
    "age_ms": sample.sampled.elapsed().as_millis().min(u64::MAX as u128) as u64,
    "interval_ms": sample.interval_ms,
    "metrics": json_value(&sample.metrics, soc, &sample.sampled_at),
  })
  .to_string()
}

fn read_path(stream: &mut TcpStream) -> Option<String> {
  let deadline = Instant::now() + Duration::from_secs(2);
  let mut header = Vec::new();
  let mut buf = [0; 1024];
  while header.len() < 8192 {
    stream.set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?)).ok()?;
    let limit = buf.len().min(8192 - header.len());
    let n = stream.read(&mut buf[..limit]).ok()?;
    if n == 0 {
      return None;
    }
    header.extend_from_slice(&buf[..n]);
    if let Some(end) = header.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
      let text = std::str::from_utf8(&header[..end]).ok()?;
      let mut line = text.lines().next()?.split_whitespace();
      if line.next()? != "GET" {
        return None;
      }
      let path = line.next()?;
      if !path.starts_with('/')
        || !matches!(line.next()?, "HTTP/1.0" | "HTTP/1.1")
        || line.next().is_some()
      {
        return None;
      }
      return Some(path.split('?').next().unwrap_or(path).to_string());
    }
  }
  None
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
  let status_text = match status {
    200 => "OK",
    404 => "Not Found",
    503 => "Service Unavailable",
    _ => "OK",
  };
  let header = format!(
    "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
    body.len()
  );
  let _ = stream.write_all(header.as_bytes()).and_then(|()| stream.write_all(body.as_bytes()));
}

fn serve_url(host: &str, port: u16) -> String {
  let host = if host.contains(':') && !host.starts_with('[') {
    format!("[{host}]")
  } else {
    host.to_string()
  };

  format!("http://{host}:{port}")
}

fn escape_xml(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
    .replace('\'', "&apos;")
}

fn handle_conn(mut stream: TcpStream, shared: SharedMetrics, soc: Arc<SocInfo>) {
  if stream.set_write_timeout(Some(Duration::from_secs(2))).is_err() {
    return;
  }
  let path = match read_path(&mut stream) {
    Some(p) => p,
    None => return,
  };

  if path == "/" {
    write_response(&mut stream, 200, "application/json", r#"{}"#);
    return;
  }

  let asset = match path.as_str() {
    "/dashboard" | "/dashboard/" => {
      Some(("text/html; charset=utf-8", include_str!("dashboard/index.html")))
    }
    "/dashboard/style.css" => {
      Some(("text/css; charset=utf-8", include_str!("dashboard/style.css")))
    }
    "/dashboard/app.js" => {
      Some(("text/javascript; charset=utf-8", include_str!("dashboard/app.js")))
    }
    _ => None,
  };
  if let Some((content_type, body)) = asset {
    write_response(&mut stream, 200, content_type, body);
    return;
  }

  let lock = shared.read().unwrap();

  let Some(sample) = lock.as_ref() else {
    drop(lock);
    write_response(&mut stream, 503, "application/json", r#"{"error":"no data yet"}"#);
    return;
  };

  match path.as_str() {
    "/json" => {
      let body = to_json(&sample.metrics, &soc);
      drop(lock);
      write_response(&mut stream, 200, "application/json", &body);
    }
    "/api/sample" => {
      let body = to_sample(sample, &soc);
      drop(lock);
      write_response(&mut stream, 200, "application/json", &body);
    }
    "/metrics" => {
      let body = to_prometheus(&sample.metrics, &soc);
      drop(lock);
      write_response(&mut stream, 200, "text/plain; version=0.0.4; charset=utf-8", &body);
    }
    _ => {
      drop(lock);
      write_response(&mut stream, 404, "application/json", r#"{"error":"not found"}"#);
    }
  }
}

pub fn launchd(host: &str, port: u16, install: bool) -> Result<(), Box<dyn std::error::Error>> {
  let home = std::env::var("HOME")?;
  let plist_path = format!("{home}/Library/LaunchAgents/com.macmon.plist");

  if !install {
    let _ = std::process::Command::new("launchctl")
      .args(["unload", &plist_path])
      .stdout(std::process::Stdio::null())
      .stderr(std::process::Stdio::null())
      .status();
    std::fs::remove_file(&plist_path)?;
    eprintln!("macmon service uninstalled");
    return Ok(());
  }

  let bin = std::env::current_exe()?;
  let bin = bin.to_string_lossy();
  let bin = escape_xml(&bin);
  let host_xml = escape_xml(host);
  let plist = format!(
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.macmon</string>
  <key>ProgramArguments</key>
  <array>
    <string>{bin}</string>
    <string>serve</string>
    <string>--host</string>
    <string>{host_xml}</string>
    <string>--port</string>
    <string>{port}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
</dict>
</plist>
"#
  );

  let agents_dir = format!("{home}/Library/LaunchAgents");
  std::fs::create_dir_all(&agents_dir)?;

  // unload silently in case it's already running
  let _ = std::process::Command::new("launchctl")
    .args(["unload", &plist_path])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status();

  std::fs::write(&plist_path, plist)?;
  std::process::Command::new("launchctl").args(["load", &plist_path]).status()?;
  eprintln!("macmon service installed: {plist_path}");
  eprintln!("configured to serve on {}", serve_url(host, port));

  Ok(())
}

pub fn run(
  host: &str,
  port: u16,
  shared: SharedMetrics,
  soc: Arc<SocInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
  let listener = TcpListener::bind((host, port))?;
  eprintln!("macmon serving on http://{}", listener.local_addr()?);
  eprintln!("  GET /dashboard/ → Local dashboard");
  eprintln!("  GET /api/sample → Latest sample with freshness metadata");
  eprintln!("  GET /json    → JSON metrics");
  eprintln!("  GET /metrics → Prometheus format");

  for stream in listener.incoming() {
    let Ok(stream) = stream else { continue };
    let shared = Arc::clone(&shared);
    let soc = Arc::clone(&soc);
    thread::spawn(move || handle_conn(stream, shared, soc));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::io::{Read, Write};
  use std::net::{Shutdown, TcpListener, TcpStream};
  use std::sync::{Arc, RwLock, mpsc};
  use std::thread;
  use std::time::{Duration, Instant};

  use macmon::{CpuTierMetrics, Metrics, NetworkMetric, SocInfo};

  use super::{
    SharedMetrics, escape_label_value, escape_xml, handle_conn, publish_sample, read_path,
    serve_url, to_json, to_prometheus, to_sample,
  };

  fn request(shared: &SharedMetrics, fragments: &[&[u8]]) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shared = Arc::clone(shared);
    let worker = thread::spawn(move || {
      handle_conn(listener.accept().unwrap().0, shared, Arc::new(SocInfo::default()));
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    client.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    for fragment in fragments {
      client.write_all(fragment).unwrap();
      thread::sleep(Duration::from_millis(5));
    }
    let _ = client.shutdown(Shutdown::Write);
    let mut response = String::new();
    if let Err(error) = client.read_to_string(&mut response) {
      assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
      assert!(response.is_empty());
    }
    worker.join().unwrap();
    response
  }

  fn get(shared: &SharedMetrics, path: &str) -> String {
    request(shared, &[format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()])
  }

  fn body(response: &str) -> &str {
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let length = headers.lines().find_map(|line| line.strip_prefix("Content-Length: ")).unwrap();
    assert_eq!(length.parse::<usize>().unwrap(), body.len());
    body
  }

  #[test]
  fn static_routes_work_without_a_sample_and_keep_the_root_json() {
    let shared = Arc::new(RwLock::new(None));
    let root = get(&shared, "/?status=1");
    assert!(root.starts_with("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"));
    assert_eq!(body(&root), "{}");
    for (path, content_type, expected) in [
      ("/dashboard", "text/html; charset=utf-8", include_str!("dashboard/index.html")),
      ("/dashboard/", "text/html; charset=utf-8", include_str!("dashboard/index.html")),
      ("/dashboard/style.css?v=1", "text/css; charset=utf-8", include_str!("dashboard/style.css")),
      ("/dashboard/app.js", "text/javascript; charset=utf-8", include_str!("dashboard/app.js")),
    ] {
      let response = get(&shared, path);
      assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{path}: {response}");
      assert!(response.contains(&format!("Content-Type: {content_type}\r\n")));
      assert!(response.contains("Cache-Control: no-store\r\n"));
      assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
      assert!(!response.contains("Access-Control-Allow-Origin"));
      assert!(!expected.is_empty());
      assert_eq!(body(&response), expected);
    }
  }

  #[test]
  fn sample_routes_return_503_until_the_first_successful_sample() {
    let shared = Arc::new(RwLock::new(None));
    for path in ["/api/sample", "/json", "/metrics"] {
      let response = get(&shared, path);
      assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
      assert_eq!(body(&response), r#"{"error":"no data yet"}"#);
    }
  }

  #[test]
  fn sample_metadata_retains_its_time_and_age_until_a_new_success() {
    let shared = Arc::new(RwLock::new(None));
    publish_sample(&shared, Metrics::default(), 1500);
    shared.write().unwrap().as_mut().unwrap().sampled = Instant::now() - Duration::from_millis(250);
    let read = || {
      let lock = shared.read().unwrap();
      serde_json::from_str::<serde_json::Value>(&to_sample(
        lock.as_ref().unwrap(),
        &SocInfo::default(),
      ))
      .unwrap()
    };
    let first = read();
    thread::sleep(Duration::from_millis(10));
    let retained = read();
    assert_eq!(first["sequence"], 1);
    assert_eq!(retained["sequence"], first["sequence"]);
    assert_eq!(retained["sampled_at"], first["sampled_at"]);
    assert_eq!(retained["metrics"], first["metrics"]);
    assert!(retained["age_ms"].as_u64().unwrap() > first["age_ms"].as_u64().unwrap());
    assert_eq!(retained["interval_ms"], 1500);
    assert_eq!(retained["metrics"]["timestamp"], retained["sampled_at"]);
    chrono::DateTime::parse_from_rfc3339(retained["sampled_at"].as_str().unwrap()).unwrap();
    publish_sample(&shared, Metrics::default(), 100);
    let next = read();
    assert_eq!(next["sequence"], 2);
    assert_eq!(next["interval_ms"], 100);
    assert!(next["age_ms"].as_u64().unwrap() < retained["age_ms"].as_u64().unwrap());
  }

  #[test]
  fn fragmented_http_requests_export_the_existing_json_shape() {
    let shared = Arc::new(RwLock::new(None));
    publish_sample(&shared, Metrics::default(), 1000);
    let response = request(
      &shared,
      &[b"GE", b"T /api/sam", b"ple?fresh=1 HTTP/1.1\r\nHost: local", b"host\r\n", b"\r\n"],
    );
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    let sample: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
    let legacy: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
    for name in
      ["soc", "cpu_tiers", "cpu_usage_pct", "ecpu_usage", "pcpu_usage", "gpu_usage", "fans"]
    {
      assert!(legacy.get(name).is_some(), "{name}");
      assert_eq!(sample["metrics"][name], legacy[name], "{name}");
    }
    assert_eq!(sample["sequence"], 1);
    let metrics = get(&shared, "/metrics");
    assert!(metrics.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body(&metrics), to_prometheus(&Metrics::default(), &SocInfo::default()));
    for path in ["/dashboard/../main.rs", "/dashboard/missing.css", "/not-found"] {
      assert!(get(&shared, path).starts_with("HTTP/1.1 404 Not Found\r\n"));
    }
  }

  #[test]
  fn request_headers_are_limited_to_eight_kibibytes_and_must_be_complete_gets() {
    let shared = Arc::new(RwLock::new(None));
    let mut exact = b"GET / HTTP/1.1\r\nHost: localhost\r\nX: ".to_vec();
    exact.resize(8192 - 4, b'x');
    exact.extend_from_slice(b"\r\n\r\n");
    assert_eq!(body(&request(&shared, &[&exact])), "{}");
    exact.insert(exact.len() - 4, b'x');
    assert!(request(&shared, &[&exact]).is_empty());
    for invalid in [
      b"GET / HTTP/1.1\r\n".as_slice(),
      b"POST /dashboard HTTP/1.1\r\n\r\n",
      b"GET / HTTP/9\r\n\r\n",
    ] {
      assert!(request(&shared, &[invalid]).is_empty());
    }
  }

  #[test]
  fn incomplete_headers_have_one_total_deadline_despite_incoming_bytes() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
      let mut stream = listener.accept().unwrap().0;
      let started = Instant::now();
      assert!(read_path(&mut stream).is_none());
      tx.send(started.elapsed()).unwrap();
    });
    let mut client = TcpStream::connect(address).unwrap();
    for byte in [b"G", b"E", b"T"] {
      client.write_all(byte).unwrap();
      thread::sleep(Duration::from_millis(600));
    }
    let elapsed = rx.recv_timeout(Duration::from_secs(4)).unwrap();
    worker.join().unwrap();
    assert!(elapsed >= Duration::from_millis(1800), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(2900), "{elapsed:?}");
  }

  #[test]
  fn exports_all_interfaces_with_escaped_labels_and_omits_unknown_rates() {
    let old = to_prometheus(&Metrics::default(), &SocInfo::default());
    let metric = NetworkMetric {
      name: "en\\\"\n0".into(),
      rx_bytes: 1u64 << 50,
      tx_bytes: 20,
      rx_bytes_per_sec: Some(1234.5),
      tx_bytes_per_sec: None,
    };
    let metrics = Metrics {
      network: Some(vec![metric, NetworkMetric { name: "utun0".into(), ..Default::default() }]),
      ..Default::default()
    };
    let output = to_prometheus(&metrics, &SocInfo::default());
    assert!(output.starts_with(&old));
    let label = r#"chip="",interface="en\\\"\n0""#;
    assert!(
      output.contains(&format!("macmon_network_receive_bytes_total{{{label}}} 1125899906842624\n"))
    );
    assert!(output.contains(&format!("macmon_network_transmit_bytes_total{{{label}}} 20\n")));
    assert!(
      output.contains(&format!("macmon_network_receive_bytes_per_second{{{label}}} 1234.5\n"))
    );
    assert!(!output.contains(&format!("macmon_network_transmit_bytes_per_second{{{label}}}")));
    assert!(
      output.contains("macmon_network_receive_bytes_total{chip=\"\",interface=\"utun0\"} 0\n")
    );
    assert_eq!(output.matches("# TYPE macmon_network_receive_bytes_total counter").count(), 1);
    assert_eq!(output.matches("# TYPE macmon_network_receive_bytes_per_second gauge").count(), 1);
    assert!(!old.contains("macmon_network_"));
    let json: serde_json::Value =
      serde_json::from_str(&to_json(&metrics, &SocInfo::default())).unwrap();
    assert_eq!(json["network"][0]["rx_bytes"].as_u64(), Some(1u64 << 50));
    assert_eq!(json["network"][0]["rx_bytes_per_sec"], 1234.5);
    assert!(json["network"][0]["tx_bytes_per_sec"].is_null());
    assert_eq!(json["network"].as_array().unwrap().len(), 2);
    assert_eq!(json["cpu_power"], 0.0);
    assert!(json.get("cpu_usage_pct").is_some());
  }

  #[test]
  fn unavailable_temperature_has_no_prometheus_gauge() {
    let mut metrics = Metrics::default();
    metrics.temp.cpu_temp_avg = Some(45.0);
    let output = to_prometheus(&metrics, &SocInfo::default());
    assert!(output.contains("macmon_cpu_temp_celsius{chip=\"\"} 45"));
    assert!(!output.contains("macmon_gpu_temp_celsius"));
    assert!(output.contains("macmon_gpu_freq_mhz"));
  }

  #[test]
  fn virtual_machine_still_exports_all_network_families() {
    let metrics = Metrics {
      network: Some(vec![NetworkMetric {
        name: "en0".into(),
        rx_bytes: 1024,
        tx_bytes: 2048,
        rx_bytes_per_sec: Some(100.0),
        tx_bytes_per_sec: Some(200.0),
      }]),
      ..Default::default()
    };
    let output = to_prometheus(&metrics, &SocInfo { vm: true, ..Default::default() });
    for direction in ["receive", "transmit"] {
      for suffix in ["bytes_total", "bytes_per_second"] {
        assert!(output.contains(&format!("macmon_network_{direction}_{suffix}{{")));
      }
    }
    assert!(!output.contains("_power_watts") && !output.contains("_freq_mhz"));
  }

  #[test]
  fn a_vm_has_no_power_and_frequency_gauges() {
    let tier = CpuTierMetrics { label: "_".into(), ..Default::default() };
    let metrics = Metrics { cpu_tiers: vec![tier], ..Default::default() };
    let types = |vm| {
      let soc = SocInfo { vm, ..Default::default() };
      let output = to_prometheus(&metrics, &soc);
      let types = output.lines().filter_map(|x| x.strip_prefix("# TYPE macmon_"));
      types.map(|x| x.trim_end_matches(" gauge").to_string()).collect::<Vec<_>>()
    };

    // a Mac: every gauge, in the same order
    let power =
      ["cpu", "gpu", "ane", "all", "sys", "ram", "gpu_ram"].map(|x| format!("{x}_power_watts"));
    let freqs = ["ecpu_freq_mhz", "pcpu_freq_mhz", "cpu_tier_freq_mhz", "gpu_freq_mhz"];
    let mac = types(false);
    assert_eq!(mac.len(), 29, "{mac:?}"); // no temperature and fan gauges without readings
    assert!(power.iter().chain(&freqs.map(String::from)).all(|x| mac.contains(x)), "{mac:?}");

    // a VM: the same without the power and frequency gauges
    let expected = mac.iter().filter(|x| !power.contains(x) && !freqs.contains(&x.as_str()));
    assert_eq!(types(true), expected.cloned().collect::<Vec<_>>());
  }

  #[test]
  fn cpu_tier_gauges_have_a_series_per_tier() {
    let tier = |label: &str, freq_mhz| CpuTierMetrics {
      label: label.into(),
      freq_mhz,
      ..Default::default()
    };
    let metrics =
      Metrics { cpu_tiers: vec![tier("E", 1800), tier("P", 3200)], ..Default::default() };
    let output = to_prometheus(&metrics, &SocInfo::default());
    assert!(output.contains("macmon_cpu_tier_freq_mhz{chip=\"\",tier=\"E\"} 1800\n"));
    assert!(output.contains("macmon_cpu_tier_freq_mhz{chip=\"\",tier=\"P\"} 3200\n"));
    assert_eq!(output.matches("# TYPE macmon_cpu_tier_active_ratio gauge").count(), 1);

    let output = to_prometheus(&Metrics::default(), &SocInfo::default());
    assert!(!output.contains("# TYPE macmon_cpu_tier_"));
  }

  #[test]
  fn formats_serving_urls() {
    assert_eq!(serve_url("127.0.0.1", 9090), "http://127.0.0.1:9090");
    assert_eq!(serve_url("0.0.0.0", 9090), "http://0.0.0.0:9090");
    assert_eq!(serve_url("::", 9090), "http://[::]:9090");
    assert_eq!(serve_url("::1", 9090), "http://[::1]:9090");
  }

  #[test]
  fn escapes_xml_values() {
    assert_eq!(escape_xml(r#"<host>&"'host"#), "&lt;host&gt;&amp;&quot;&apos;host");
  }

  #[test]
  fn escapes_prometheus_label_values() {
    assert_eq!(escape_label_value("Mac\\Book\n\"Pro\""), r#"Mac\\Book\n\"Pro\""#);
  }

  #[test]
  fn includes_usage_fields_in_json() {
    let json: serde_json::Value =
      serde_json::from_str(&to_json(&Metrics::default(), &SocInfo::default())).unwrap();

    for field in ["cpu_tiers", "cpu_usage_pct", "ecpu_usage", "pcpu_usage", "gpu_usage"] {
      assert!(json.get(field).is_some(), "missing {field}");
    }
  }
}
