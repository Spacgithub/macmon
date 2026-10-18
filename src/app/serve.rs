//! HTTP serving and launchd support.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use macmon::{DiskMetric, MemoryActivity, Metrics, SocInfo, TemperatureReading, ThermalState};

use crate::config::{TUI_MAX_MS, TUI_MIN_MS};
use crate::metrics_to_json_value;
#[cfg(test)]
use crate::procs::ProcessSnapshot;

pub type SharedMetrics = Arc<RwLock<Option<Arc<Sample>>>>;
pub type SharedCadence = Arc<RwLock<Cadence>>;

pub struct Cadence {
  requested_ms: u32,
  effective_ms: u32,
  generation: u64,
  source: Option<Arc<crate::source::Client>>,
}

impl Cadence {
  pub fn new(initial_ms: u32) -> Self {
    Self { requested_ms: initial_ms, effective_ms: initial_ms, generation: 0, source: None }
  }

  pub fn begin(&mut self) -> u32 {
    if self.effective_ms != self.requested_ms {
      self.effective_ms = self.requested_ms;
      self.generation = self.generation.saturating_add(1);
    }
    self.effective_ms
  }

  pub(crate) fn adjust(&mut self, adjustment: &str) -> bool {
    let requested_ms = match adjustment {
      "increase" => self.requested_ms.saturating_add(250) / 250 * 250,
      "decrease" => self.requested_ms.saturating_sub(250).div_ceil(250) * 250,
      _ => return false,
    }
    .clamp(TUI_MIN_MS, TUI_MAX_MS);
    self.set(requested_ms);
    true
  }

  pub(crate) fn state(&self) -> crate::source::CadenceState {
    if let Some(source) = &self.source {
      return source.cadence();
    }
    crate::source::CadenceState {
      requested_ms: self.requested_ms,
      effective_ms: self.effective_ms,
      pending: self.requested_ms != self.effective_ms,
      generation: self.generation,
    }
  }
  pub(crate) fn set(&mut self, interval_ms: u32) {
    if self.requested_ms != interval_ms {
      self.requested_ms = interval_ms;
      self.generation = self.generation.saturating_add(1);
    }
  }
  pub(crate) fn with_source(source: Arc<crate::source::Client>) -> Self {
    Self { source: Some(source), ..Self::new(1000) }
  }
  fn value(&self) -> serde_json::Value {
    cadence_value(self.state())
  }
}

// Ordering metadata belongs to private IPC; every HTTP cadence keeps its released schema.
fn cadence_value(state: crate::source::CadenceState) -> serde_json::Value {
  serde_json::json!({"requested_ms": state.requested_ms, "effective_ms": state.effective_ms,
    "pending": state.pending})
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SampleDetails {
  pub temperatures: Vec<TemperatureReading>,
  pub memory_activity: MemoryActivity,
  pub thermal_state: Option<ThermalState>,
  pub disks: Option<Vec<DiskMetric>>,
}

impl From<Vec<TemperatureReading>> for SampleDetails {
  fn from(temperatures: Vec<TemperatureReading>) -> Self {
    Self { temperatures, ..Self::default() }
  }
}

pub(crate) type Sample = crate::source::Frame;

/// Capture system-metric completion before collecting the separate process snapshot.
pub fn sample_time() -> (Instant, String) {
  (Instant::now(), chrono::Utc::now().to_rfc3339())
}

#[cfg(test)]
pub(crate) fn publish_sample(
  shared: &SharedMetrics,
  metrics: Metrics,
  details: impl Into<SampleDetails>,
  interval_ms: u32,
  processes: Option<ProcessSnapshot>,
  time: (Instant, String),
) {
  let (sampled, sampled_at) = time;
  let mut latest = shared.write().unwrap();
  let sequence = latest.as_ref().map_or(1, |sample| sample.sequence.saturating_add(1));
  *latest = Some(Arc::new(Sample {
    session: "fixture".into(),
    revision: sequence,
    collector_pid: std::process::id(),
    soc: SocInfo::default(),
    cadence: crate::source::CadenceState {
      requested_ms: interval_ms,
      effective_ms: interval_ms,
      pending: false,
      generation: 0,
    },
    process_interval_ms: interval_ms,
    process_state: if processes.is_some() {
      crate::source::ProcessState::Ready
    } else {
      crate::source::ProcessState::Warming
    },
    metrics,
    details: details.into(),
    sequence,
    sampled_at,
    sampled,
    interval_ms,
    processes: processes.map(|snapshot| Arc::new(crate::source::ProcessFrame::fixture(snapshot))),
  }));
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

fn to_sample(sample: &Sample, soc: &SocInfo, local: bool, cadence: &Cadence) -> String {
  let processes = sample.processes.as_ref().filter(|_| local);
  serde_json::json!({
    "sequence": sample.sequence,
    "sampled_at": sample.sampled_at,
    "age_ms": sample.sampled.elapsed().as_millis().min(u64::MAX as u128) as u64,
    "interval_ms": sample.interval_ms,
    "cadence": cadence.value(),
    "metrics": json_value(&sample.metrics, soc, &sample.sampled_at),
    "temperatures": sample.details.temperatures,
    "memory_activity": sample.details.memory_activity,
    "thermal_state": sample.details.thermal_state.filter(|_| !soc.vm),
    "disks": sample.details.disks,
    "processes": processes.map(|processes| &processes.snapshot.rows),
    "process_groups": processes.map(|processes| &processes.snapshot.groups),
    "processes_age_ms": processes.map(|processes| processes.snapshot.sampled.elapsed().as_millis().min(u64::MAX as u128) as u64),
  })
  .to_string()
}

struct Request {
  method: String,
  path: String,
  headers: HashMap<String, String>,
  body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
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
      let method = line.next()?;
      if !matches!(method, "GET" | "POST") {
        return None;
      }
      let path = line.next()?;
      if !path.starts_with('/')
        || !matches!(line.next()?, "HTTP/1.0" | "HTTP/1.1")
        || line.next().is_some()
      {
        return None;
      }
      let mut headers = HashMap::new();
      for field in text.lines().skip(1) {
        let (name, value) = field.split_once(':')?;
        let name = name.to_ascii_lowercase();
        if matches!(
          name.as_str(),
          "host" | "origin" | "content-type" | "content-length" | "transfer-encoding"
        ) && headers.insert(name, value.trim().to_string()).is_some()
        {
          return None;
        }
      }
      let mut request = Request {
        method: method.to_string(),
        path: path.split('?').next()?.to_string(),
        headers,
        body: header[end + 4..].to_vec(),
      };
      if method == "POST" {
        let length: usize = request.headers.get("content-length")?.parse().ok()?;
        if length > 64 || request.headers.contains_key("transfer-encoding") {
          return None;
        }
        while request.body.len() < length {
          stream.set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?)).ok()?;
          let n = stream.read(&mut buf[..(length - request.body.len()).min(1024)]).ok()?;
          if n == 0 {
            return None;
          }
          request.body.extend_from_slice(&buf[..n]);
        }
        if request.body.len() != length {
          return None;
        }
      }
      return Some(request);
    }
  }
  None
}

fn loopback(ip: std::net::IpAddr) -> bool {
  ip.to_canonical().is_loopback()
}

fn loopback_host(peer: SocketAddr, port: u16, host: &str) -> bool {
  let address = host
    .parse::<SocketAddr>()
    .ok()
    .or_else(|| (port == 80).then(|| format!("{host}:80").parse::<SocketAddr>().ok()).flatten());
  loopback(peer.ip())
    && (host.eq_ignore_ascii_case(&format!("localhost:{port}"))
      || port == 80 && host.eq_ignore_ascii_case("localhost")
      || address.is_some_and(|address| loopback(address.ip()) && address.port() == port))
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
  let status_text = match status {
    200 => "OK",
    400 => "Bad Request",
    403 => "Forbidden",
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

fn handle_conn(
  mut stream: TcpStream,
  shared: SharedMetrics,
  soc: Arc<SocInfo>,
  cadence: SharedCadence,
) {
  if stream.set_write_timeout(Some(Duration::from_secs(2))).is_err() {
    return;
  }
  let request = match read_request(&mut stream) {
    Some(p) => p,
    None => return,
  };
  let path = &request.path;
  let host = request.headers.get("host").map(String::as_str).unwrap_or("");
  let local = stream
    .peer_addr()
    .ok()
    .zip(stream.local_addr().ok())
    .is_some_and(|(peer, local)| loopback_host(peer, local.port(), host));
  if request.method == "POST" {
    let allowed = local
      && request.headers.get("origin").is_some_and(|origin| origin == &format!("http://{host}"))
      && request.headers.get("content-type").is_some_and(|kind| {
        kind.split(';').next().unwrap().trim().eq_ignore_ascii_case("application/json")
      });
    let body = serde_json::from_slice::<serde_json::Value>(&request.body).unwrap_or_default();
    let status = if path != "/api/interval" {
      404
    } else if !allowed {
      403
    } else {
      200
    };
    let (status, body) = if status == 200 {
      let mut control = cadence.write().unwrap();
      let adjustment = body["adjustment"].as_str().unwrap_or("");
      if let Some(source) = &control.source {
        match source.adjust(adjustment) {
          Ok(value) => (200, cadence_value(value).to_string()),
          Err(_) if matches!(adjustment, "increase" | "decrease") => {
            (503, r#"{"error":"sampling source unavailable"}"#.into())
          }
          Err(_) => (400, r#"{"error":"invalid adjustment"}"#.into()),
        }
      } else if control.adjust(adjustment) {
        (200, control.value().to_string())
      } else {
        (400, r#"{"error":"invalid adjustment"}"#.to_string())
      }
    } else {
      (status, r#"{"error":"local same-origin POST required"}"#.to_string())
    };
    write_response(&mut stream, status, "application/json", &body);
    return;
  }

  if path == "/api/source" {
    let control = cadence.read().unwrap();
    let source = control.source.as_ref();
    let frame = source.and_then(|source| source.latest());
    let body = serde_json::json!({"protocol": 1, "session": frame.as_ref().map(|frame| &frame.session),
      "collector_pid": frame.as_ref().map(|frame| frame.collector_pid),
      "connected": source.is_some_and(|source| source.status() == crate::source::ConnectionState::Live)}).to_string();
    write_response(
      &mut stream,
      if local { 200 } else { 403 },
      "application/json",
      if local { &body } else { "{}" },
    );
    return;
  }

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
      let body = to_sample(sample, &soc, local, &cadence.read().unwrap());
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
  listener: TcpListener,
  shared: SharedMetrics,
  soc: Arc<SocInfo>,
  cadence: SharedCadence,
) -> Result<(), Box<dyn std::error::Error>> {
  eprintln!("macmon serving on http://{}", listener.local_addr()?);
  eprintln!("  GET /dashboard/ → Local dashboard");
  eprintln!("  GET /api/sample → Latest sample with freshness metadata");
  eprintln!("  POST /api/interval → Same-origin loopback sampling control");
  eprintln!("  GET /json    → JSON metrics");
  eprintln!("  GET /metrics → Prometheus format");

  for stream in listener.incoming() {
    let Ok(stream) = stream else { continue };
    let shared = Arc::clone(&shared);
    let soc = Arc::clone(&soc);
    let cadence = Arc::clone(&cadence);
    thread::spawn(move || handle_conn(stream, shared, soc, cadence));
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

  use macmon::{
    CpuTierMetrics, Metrics, NetworkMetric, SocInfo, TempMetrics, TemperatureGroup,
    TemperatureReading, TemperatureSource,
  };

  use super::{
    Cadence, SharedCadence, SharedMetrics, escape_label_value, escape_xml, handle_conn,
    loopback_host, publish_sample, read_request, serve_url, to_json, to_prometheus, to_sample,
  };

  fn request(shared: &SharedMetrics, fragments: &[&[u8]]) -> String {
    exchange(shared, &Arc::new(RwLock::new(Cadence::new(1000))), |_| {
      fragments.iter().map(|part| part.to_vec()).collect()
    })
  }

  fn exchange(
    shared: &SharedMetrics,
    cadence: &SharedCadence,
    build: impl FnOnce(u16) -> Vec<Vec<u8>>,
  ) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shared = Arc::clone(shared);
    let cadence = Arc::clone(cadence);
    let worker = thread::spawn(move || {
      handle_conn(listener.accept().unwrap().0, shared, Arc::new(SocInfo::default()), cadence);
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    client.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    for fragment in build(address.port()) {
      client.write_all(&fragment).unwrap();
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

  fn adjustment(shared: &SharedMetrics, cadence: &SharedCadence, action: &str) -> String {
    exchange(shared, cadence, |port| {
      let body = format!(r#"{{"adjustment":"{action}"}}"#);
      let header = format!(
        "POST /api/interval?control=1 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
      );
      vec![header.as_bytes()[..17].to_vec(), header.as_bytes()[17..].to_vec(), body.into_bytes()]
    })
  }

  fn body(response: &str) -> &str {
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let length = headers.lines().find_map(|line| line.strip_prefix("Content-Length: ")).unwrap();
    assert_eq!(length.parse::<usize>().unwrap(), body.len());
    body
  }

  #[test]
  fn cadence_generation_orders_changes_without_entering_any_http_cadence() {
    let mut control = Cadence::new(1000);
    assert_eq!(control.state().generation, 0);
    assert_eq!(control.begin(), 1000);
    assert_eq!(control.state().generation, 0);
    assert!(control.adjust("increase"));
    assert_eq!(control.state().generation, 1);
    control.set(1250);
    assert_eq!(control.state().generation, 1, "an unchanged request adds no generation");
    let pending = serde_json::json!({"requested_ms":1250,"effective_ms":1000,"pending":true});
    assert_eq!(super::cadence_value(control.state()), pending, "shared-control HTTP response");
    assert_eq!(control.value(), pending);
    let shared = Arc::new(RwLock::new(None));
    publish_sample(&shared, Metrics::default(), vec![], 1000, None, super::sample_time());
    let sample: serde_json::Value = serde_json::from_str(&to_sample(
      shared.read().unwrap().as_ref().unwrap(),
      &SocInfo::default(),
      true,
      &control,
    ))
    .unwrap();
    assert_eq!(sample["cadence"], pending, "GET /api/sample keeps its three fields");
    let control = Arc::new(RwLock::new(control));
    let response = adjustment(&shared, &control, "increase");
    assert_eq!(
      serde_json::from_str::<serde_json::Value>(body(&response)).unwrap(),
      serde_json::json!({"requested_ms":1500,"effective_ms":1000,"pending":true}),
      "POST /api/interval keeps its three fields"
    );
    let mut control = control.write().unwrap();
    assert_eq!(control.state().generation, 2);
    assert_eq!(control.begin(), 1500);
    assert_eq!(control.state().generation, 3);
    assert_eq!(control.begin(), 1500);
    assert_eq!(control.state().generation, 3, "an unchanged window adds no generation");
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
  fn named_temperatures_share_the_metrics_sample_and_replace_previous_sources() {
    let shared = Arc::new(RwLock::new(None));
    for (sequence, source, name, celsius) in [
      (1, TemperatureSource::Smc, "Tp01", 45.0),
      (2, TemperatureSource::Hid, "pACC MTR Temp Sensor0", 50.0),
    ] {
      let metrics = Metrics {
        temp: TempMetrics { cpu_temp_avg: Some(celsius), gpu_temp_avg: None },
        ..Default::default()
      };
      let expected_prometheus = to_prometheus(&metrics, &SocInfo::default());
      let temperatures = vec![TemperatureReading {
        name: name.into(),
        group: TemperatureGroup::Cpu,
        source,
        celsius,
      }];
      let expected = serde_json::to_value(&temperatures).unwrap();
      publish_sample(&shared, metrics, temperatures, 1000, None, super::sample_time());
      let response: serde_json::Value =
        serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
      assert_eq!(response["sequence"], sequence);
      assert_eq!(response["temperatures"], expected);
      assert_eq!(
        response["metrics"]["temp"],
        serde_json::json!({
          "cpu_temp_avg": celsius, "gpu_temp_avg": null
        })
      );
      assert_eq!(response["metrics"]["timestamp"], response["sampled_at"]);
      let legacy: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
      assert_eq!(legacy["temp"], response["metrics"]["temp"]);
      assert!(legacy.get("temperatures").is_none());
      assert!(response["metrics"].get("temperatures").is_none());
      assert_eq!(body(&get(&shared, "/metrics")), expected_prometheus);
    }
    publish_sample(&shared, Metrics::default(), Vec::new(), 1000, None, super::sample_time());
    let response: serde_json::Value =
      serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
    assert_eq!(response["sequence"], 3);
    assert_eq!(response["temperatures"], serde_json::json!([]));
    assert!(response["metrics"]["temp"]["cpu_temp_avg"].is_null());
  }

  #[test]
  fn memory_activity_is_sample_scoped_and_leaves_legacy_outputs_unchanged() {
    let shared = Arc::new(RwLock::new(None));
    let expected_prometheus = to_prometheus(&Metrics::default(), &SocInfo::default());
    let activity = macmon::MemoryActivity {
      pressure: Some(macmon::MemoryPressure::Warning),
      swap_in_bytes_per_sec: Some(16384.0),
      swap_out_bytes_per_sec: Some(0.0),
    };
    for (index, activity) in [activity, macmon::MemoryActivity::default()].into_iter().enumerate() {
      let details = super::SampleDetails { memory_activity: activity, ..Default::default() };
      publish_sample(&shared, Metrics::default(), details, 1000, None, super::sample_time());
      let response: serde_json::Value =
        serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
      assert_eq!(response["sequence"], index + 1);
      assert_eq!(response["memory_activity"], serde_json::to_value(activity).unwrap());
      assert!(response["metrics"].get("memory_activity").is_none());
      let legacy: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
      assert!(legacy.get("memory_activity").is_none());
      assert_eq!(legacy["memory"], response["metrics"]["memory"]);
      assert_eq!(legacy["memory"].as_object().unwrap().len(), 4);
      assert_eq!(body(&get(&shared, "/metrics")), expected_prometheus);
    }
  }

  #[test]
  fn thermal_states_share_the_sample_and_keep_legacy_outputs_and_vm_fallbacks() {
    use macmon::ThermalState::{Critical, Fair, Nominal, Serious};

    let shared = Arc::new(RwLock::new(None));
    let soc = SocInfo::default();
    let cadence = Cadence::new(1000);
    let expected_prometheus = to_prometheus(&Metrics::default(), &soc);
    for (index, state) in
      [Some(Nominal), Some(Fair), Some(Serious), Some(Critical), None].into_iter().enumerate()
    {
      let details = super::SampleDetails { thermal_state: state, ..Default::default() };
      publish_sample(&shared, Metrics::default(), details, 1000, None, super::sample_time());
      let response: serde_json::Value =
        serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
      assert_eq!(response["sequence"], index + 1);
      assert_eq!(response["thermal_state"], serde_json::to_value(state).unwrap());
      assert!(response["metrics"].get("thermal_state").is_none());
      let legacy: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
      assert!(legacy.get("thermal_state").is_none());
      assert_eq!(body(&get(&shared, "/metrics")), expected_prometheus);
      let lock = shared.read().unwrap();
      let sample = lock.as_ref().unwrap();
      let vm = SocInfo { vm: true, ..Default::default() };
      for local in [true, false] {
        let response: serde_json::Value =
          serde_json::from_str(&to_sample(sample, &vm, local, &cadence)).unwrap();
        assert!(response["thermal_state"].is_null());
      }
    }
    publish_sample(&shared, Metrics::default(), Vec::new(), 1000, None, super::sample_time());
    let response: serde_json::Value =
      serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
    assert!(response["thermal_state"].is_null());
  }

  #[test]
  fn disk_snapshots_distinguish_unavailable_empty_and_lossless_health_without_legacy_changes() {
    use macmon::{DiskHealth, DiskHealthState, DiskMetric, NvmeHealth};

    let shared = Arc::new(RwLock::new(None));
    let expected_prometheus = to_prometheus(&Metrics::default(), &SocInfo::default());
    let disk = DiskMetric {
      id: "123:456".into(),
      name: "disk0".into(),
      read_bytes: Some(0),
      read_bytes_per_sec: Some(0.0),
      write_bytes_per_sec: None,
      health: DiskHealth {
        state: DiskHealthState::Available,
        age_ms: Some(6000),
        nvme: Some(NvmeHealth {
          data_units_written: u128::MAX.to_string(),
          percentage_used: 201,
          ..Default::default()
        }),
      },
      ..Default::default()
    };
    for (index, disks) in [Some(vec![disk]), None, Some(vec![])].into_iter().enumerate() {
      let expected = serde_json::to_value(&disks).unwrap();
      let details = super::SampleDetails { disks, ..Default::default() };
      publish_sample(&shared, Metrics::default(), details, 1000, None, super::sample_time());
      let response: serde_json::Value =
        serde_json::from_str(body(&get(&shared, "/api/sample"))).unwrap();
      assert_eq!(response["sequence"], index + 1);
      assert_eq!(response["disks"], expected);
      assert!(response["metrics"].get("disks").is_none());
      let legacy: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
      assert!(legacy.get("disks").is_none());
      assert_eq!(body(&get(&shared, "/metrics")), expected_prometheus);
    }
  }

  #[test]
  fn sample_metadata_retains_its_time_and_age_until_a_new_success() {
    let shared = Arc::new(RwLock::new(None));
    publish_sample(&shared, Metrics::default(), Vec::new(), 1500, None, super::sample_time());
    Arc::get_mut(shared.write().unwrap().as_mut().unwrap()).unwrap().sampled =
      Instant::now() - Duration::from_millis(250);
    let read = || {
      let lock = shared.read().unwrap();
      serde_json::from_str::<serde_json::Value>(&to_sample(
        lock.as_ref().unwrap(),
        &SocInfo::default(),
        false,
        &Cadence::new(1500),
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
    publish_sample(&shared, Metrics::default(), Vec::new(), 100, None, super::sample_time());
    let next = read();
    assert_eq!(next["sequence"], 2);
    assert_eq!(next["interval_ms"], 100);
    assert!(next["age_ms"].as_u64().unwrap() < retained["age_ms"].as_u64().unwrap());
  }

  #[test]
  fn system_sample_time_includes_a_slow_process_sweep() {
    let shared = Arc::new(RwLock::new(None));
    let time = super::sample_time();
    let instant = time.0;
    let timestamp = time.1.clone();
    thread::sleep(Duration::from_millis(25)); // Stand in for process enumeration.
    let processes = crate::procs::ProcessSnapshot {
      rows: Vec::new(),
      groups: Vec::new(),
      sampled: Instant::now(),
    };
    publish_sample(&shared, Metrics::default(), Vec::new(), 1000, Some(processes), time);
    let lock = shared.read().unwrap();
    let sample = lock.as_ref().unwrap();
    let json: serde_json::Value =
      serde_json::from_str(&to_sample(sample, &SocInfo::default(), true, &Cadence::new(1000)))
        .unwrap();
    assert_eq!(sample.sampled, instant);
    assert_eq!(json["sampled_at"], timestamp);
    assert_eq!(json["metrics"]["timestamp"], timestamp);
    assert!(json["age_ms"].as_u64().unwrap() >= 25);
    assert!(json["processes_age_ms"].as_u64().unwrap() < json["age_ms"].as_u64().unwrap());
  }

  #[test]
  fn loopback_peers_and_host_authorities_canonicalize_mapped_ipv4() {
    let peer = "[::ffff:127.0.0.1]:54321".parse().unwrap();
    for ip in ["127.0.0.1", "127.12.0.1", "::1", "::ffff:127.0.0.1"] {
      assert!(super::loopback(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["192.0.2.1", "::", "::ffff:192.0.2.1"] {
      assert!(!super::loopback(ip.parse().unwrap()), "{ip}");
    }
    for host in ["localhost:9090", "127.0.0.1:9090", "[::1]:9090", "[::ffff:127.0.0.1]:9090"] {
      assert!(loopback_host(peer, 9090, host), "{host}");
    }
    for host in ["localhost", "127.0.0.1", "[::1]", "[::ffff:127.0.0.1]"] {
      assert!(loopback_host(peer, 80, host), "{host}");
      assert!(!loopback_host(peer, 9090, host), "unported {host}");
    }
    for host in ["[::ffff:192.0.2.1]:9090", "[::ffff:127.0.0.1]:80", "foreign.example:9090"] {
      assert!(!loopback_host(peer, 9090, host), "{host}");
    }
    assert!(!loopback_host("[::ffff:192.0.2.1]:54321".parse().unwrap(), 9090, "localhost:9090"));
  }

  #[test]
  fn ipv6_and_available_dual_stack_connections_keep_local_processes() {
    let shared = Arc::new(RwLock::new(None));
    let processes = crate::procs::ProcessSnapshot {
      rows: Vec::new(),
      groups: Vec::new(),
      sampled: Instant::now(),
    };
    publish_sample(
      &shared,
      Metrics::default(),
      Vec::new(),
      1000,
      Some(processes),
      super::sample_time(),
    );
    for (bind, connect, host) in
      [("::1", "::1", "[::1]"), ("::", "127.0.0.1", "[::ffff:127.0.0.1]")]
    {
      let listener = match TcpListener::bind((bind, 0)) {
        Ok(listener) => listener,
        Err(error)
          if matches!(
            error.kind(),
            std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
          ) =>
        {
          eprintln!("IPv6 unavailable: {error}");
          continue;
        }
        Err(error) => panic!("IPv6 bind: {error}"),
      };
      let port = listener.local_addr().unwrap().port();
      let mut client = match TcpStream::connect_timeout(
        &std::net::SocketAddr::new(connect.parse().unwrap(), port),
        Duration::from_secs(1),
      ) {
        Ok(client) => client,
        Err(error) if bind == "::" && error.kind() == std::io::ErrorKind::ConnectionRefused => {
          eprintln!("IPv4 mapping unavailable on this IPv6 listener");
          continue;
        }
        Err(error) => panic!("IPv6 connect: {error}"),
      };
      let (stream, peer) = listener.accept().unwrap();
      eprintln!("Validated local process disclosure for peer {peer}");
      assert!(super::loopback(peer.ip()));
      let latest = Arc::clone(&shared);
      let worker = thread::spawn(move || {
        handle_conn(
          stream,
          latest,
          Arc::new(SocInfo::default()),
          Arc::new(RwLock::new(Cadence::new(1000))),
        )
      });
      client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
      write!(client, "GET /api/sample HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n").unwrap();
      let mut response = String::new();
      client.read_to_string(&mut response).unwrap();
      worker.join().unwrap();
      let json: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
      assert!(json["processes"].is_array());
    }
  }

  #[test]
  fn group_snapshot_keeps_flat_rows_totals_privacy_and_retained_identity() {
    use crate::proc_groups::{AppIdentity, ProcMeta, groups};
    use crate::procs::{ProcInfo, ProcSampler, ProcessSnapshot};
    let app = AppIdentity { id: "opaque-app".into(), name: "Browser".into() };
    let row = |pid, instance: &str, cpu, gpu, known| ProcInfo {
      pid,
      name: "worker".into(),
      path: "/private/never-published/Browser.app/Contents/MacOS/worker".into(),
      user: "private-user".into(),
      cpu_pct: cpu,
      gpu_pct: gpu,
      cpu_time_ns: 20,
      mem_bytes: 64,
      power_w: Some(2.0),
      started: Some((42, 0)),
      meta: ProcMeta { instance: instance.into(), app: Some(app.clone()), known },
    };
    let rows = vec![
      row(20, "opaque-one", 125.0, 0.0, (true, true)),
      row(21, "opaque-two", 0.0, 9.0, (false, false)),
    ];
    let snapshot = ProcessSnapshot {
      groups: groups(&rows),
      rows: ProcSampler::dashboard_rows(rows),
      sampled: Instant::now() - Duration::from_secs(5),
    };
    let shared = Arc::new(RwLock::new(None));
    publish_sample(
      &shared,
      Metrics::default(),
      Vec::new(),
      1000,
      Some(snapshot),
      super::sample_time(),
    );
    let lock = shared.read().unwrap();
    let sample = lock.as_ref().unwrap();
    let value = |local| {
      serde_json::from_str::<serde_json::Value>(&to_sample(
        sample,
        &SocInfo::default(),
        local,
        &Cadence::new(1000),
      ))
      .unwrap()
    };
    let local = value(true);
    let retained = value(true);
    assert_eq!(local["sequence"], retained["sequence"]);
    assert_eq!(local["process_groups"], retained["process_groups"]);
    assert!(local["processes_age_ms"].as_u64().unwrap() >= 5000);
    assert_eq!(local["processes"][0].as_object().unwrap().len(), 5);
    let group = &local["process_groups"][0];
    assert_eq!(group["kind"], "application");
    assert_eq!(group["members"].as_array().unwrap().len(), 2);
    assert_eq!((group["cpu_pct"].as_f64(), group["gpu_pct"].as_f64()), (Some(125.0), Some(0.0)));
    assert_eq!((group["cpu_known"].as_u64(), group["gpu_known"].as_u64()), (Some(1), Some(1)));
    assert_eq!(group["mem_bytes"], 128);
    assert!(local["processes"][1]["cpu_pct"].is_null());
    assert!(local["processes"][1]["gpu_pct"].is_null());
    let serialized = local.to_string();
    for private in ["/private/never-published", "private-user", "power_known", "cpu_time_ns"] {
      assert!(!serialized.contains(private), "{private}");
    }
    let remote = value(false);
    assert!(remote["process_groups"].is_null() && remote["processes"].is_null());
    assert!(remote["processes_age_ms"].is_null());
  }

  #[test]
  fn process_snapshots_require_loopback_authority_and_keep_their_own_age() {
    #[cfg(test)]
    use crate::procs::ProcessSnapshot;
    let shared = Arc::new(RwLock::new(None));
    publish_sample(
      &shared,
      Metrics::default(),
      Vec::new(),
      1000,
      Some(ProcessSnapshot {
        rows: Vec::new(),
        groups: Vec::new(),
        sampled: Instant::now() - Duration::from_secs(5),
      }),
      super::sample_time(),
    );
    for (authority, allowed) in [
      ("localhost:PORT", true),
      ("127.0.0.1:PORT", true),
      ("[::1]:PORT", true),
      ("attacker.example:PORT", false),
      ("localhost:0", false),
      ("127.0.0.1.attacker:PORT", false),
      ("localhost:PORT\r\nHost: localhost:PORT", false),
    ] {
      let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
      let address = listener.local_addr().unwrap();
      let latest = Arc::clone(&shared);
      let worker = thread::spawn(move || {
        handle_conn(
          listener.accept().unwrap().0,
          latest,
          Arc::new(SocInfo::default()),
          Arc::new(RwLock::new(Cadence::new(1000))),
        )
      });
      let mut client = TcpStream::connect(address).unwrap();
      client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
      let host = authority.replace("PORT", &address.port().to_string());
      write!(client, "GET /api/sample HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
      let mut response = String::new();
      client.read_to_string(&mut response).unwrap();
      worker.join().unwrap();
      if authority.contains("\r\n") {
        assert!(response.is_empty(), "Duplicate Host must be rejected before routing");
        continue;
      }
      let json: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
      assert_eq!(json["processes"].is_array(), allowed, "{authority}");
      assert_eq!(json["process_groups"].is_array(), allowed, "{authority}");
      if allowed {
        assert!(json["processes_age_ms"].as_u64().unwrap() >= 5000);
      } else {
        assert!(json["processes_age_ms"].is_null());
      }
      assert_eq!(json["sequence"], 1);
    }
    let lock = shared.read().unwrap();
    let remote: serde_json::Value = serde_json::from_str(&to_sample(
      lock.as_ref().unwrap(),
      &SocInfo::default(),
      false,
      &Cadence::new(1000),
    ))
    .unwrap();
    assert!(remote["processes"].is_null() && remote["processes_age_ms"].is_null());
    assert!(remote["process_groups"].is_null());
    let old: serde_json::Value = serde_json::from_str(body(&get(&shared, "/json"))).unwrap();
    assert!(old.get("processes").is_none() && old.get("processes_age_ms").is_none());
    assert!(old.get("process_groups").is_none());
    assert_eq!(
      body(&get(&shared, "/metrics")),
      to_prometheus(&Metrics::default(), &SocInfo::default())
    );
  }

  #[test]
  fn fragmented_http_requests_export_the_existing_json_shape() {
    let shared = Arc::new(RwLock::new(None));
    publish_sample(&shared, Metrics::default(), Vec::new(), 1000, None, super::sample_time());
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
      assert!(read_request(&mut stream).is_none());
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
  fn interval_adjustments_match_terminal_steps_and_keep_cli_initial_values() {
    for (initial, action, expected) in [
      (100, "increase", 250),
      (100, "decrease", 250),
      (1000, "increase", 1250),
      (1000, "decrease", 750),
      (999, "increase", 1000),
      (999, "decrease", 750),
      (250, "decrease", 250),
      (10_000, "increase", 10_000),
      (u32::MAX, "increase", 10_000),
    ] {
      let mut cadence = Cadence::new(initial);
      assert_eq!(cadence.begin(), initial, "CLI interval stays compatible");
      assert!(cadence.adjust(action));
      assert_eq!(cadence.value()["requested_ms"], expected);
      assert_eq!(cadence.value()["effective_ms"], initial);
      assert_eq!(cadence.begin(), expected);
      assert_eq!(cadence.value()["pending"], false);
      assert!(!cadence.adjust("set"));
      assert_eq!(cadence.begin(), expected);
    }
  }

  #[test]
  fn interval_host_validation_uses_loopback_peer_literal_host_and_actual_port() {
    let peer = "127.0.0.1:54321".parse().unwrap();
    for host in ["127.0.0.1:9090", "127.42.0.1:9090", "localhost:9090", "[::1]:9090"] {
      assert!(loopback_host(peer, 9090, host), "{host}");
    }
    for host in [
      "",
      "localhost",
      "127.0.0.1",
      "127.0.0.1:9091",
      "localhost:9091",
      "example.com:9090",
      "127.0.0.1.example.com:9090",
      "localhost.:9090",
      "user@localhost:9090",
      "0.0.0.0:9090",
      "[::]:9090",
      "192.168.1.1:9090",
    ] {
      assert!(!loopback_host(peer, 9090, host), "{host}");
    }
    for host in ["localhost", "127.0.0.1", "[::1]", "127.0.0.1:80"] {
      assert!(loopback_host(peer, 80, host), "{host}");
    }
    assert!(!loopback_host("192.168.1.1:54321".parse().unwrap(), 9090, "localhost:9090"));
    assert!(loopback_host("[::1]:54321".parse().unwrap(), 9090, "[::1]:9090"));
  }

  #[test]
  fn local_json_post_changes_only_the_next_window_and_allows_repeated_adjustments() {
    let shared = Arc::new(RwLock::new(None));
    let cadence = Arc::new(RwLock::new(Cadence::new(100)));
    let response = adjustment(&shared, &cadence, "increase");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(!response.contains("Access-Control-Allow-Origin"));
    let pending: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
    assert_eq!(pending, serde_json::json!({"requested_ms":250,"effective_ms":100,"pending":true}));
    adjustment(&shared, &cadence, "increase");
    assert_eq!(cadence.read().unwrap().value()["requested_ms"], 500);
    assert_eq!(cadence.read().unwrap().value()["effective_ms"], 100);
    assert_eq!(cadence.write().unwrap().begin(), 500);
    let invalid = adjustment(&shared, &cadence, "set");
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert_eq!(cadence.read().unwrap().value()["requested_ms"], 500);
  }

  #[test]
  fn interval_post_rejects_cross_origin_rebinding_and_ambiguous_requests() {
    let shared = Arc::new(RwLock::new(None));
    let cadence = Arc::new(RwLock::new(Cadence::new(1000)));
    for mutation in [
      "missing-origin",
      "foreign-origin",
      "null-origin",
      "wrong-port",
      "rebinding-host",
      "missing-type",
      "form-type",
      "duplicate-host",
      "duplicate-origin",
      "transfer-encoding",
      "oversized-body",
      "truncated-body",
      "no-length",
      "bad-body",
      "get",
    ] {
      let response = exchange(&shared, &cadence, |port| {
        let mut host = format!("127.0.0.1:{port}");
        let mut origin = format!("Origin: http://{host}\r\n");
        let mut kind = "Content-Type: application/json\r\n";
        let mut extra = String::new();
        let mut payload = r#"{"adjustment":"decrease"}"#.to_string();
        let mut length = payload.len().to_string();
        match mutation {
          "missing-origin" => origin.clear(),
          "foreign-origin" => origin = "Origin: https://attacker.invalid\r\n".into(),
          "null-origin" => origin = "Origin: null\r\n".into(),
          "wrong-port" => host = format!("127.0.0.1:{}", port.saturating_sub(1)),
          "rebinding-host" => {
            host = format!("localhost.attacker.invalid:{port}");
            origin = format!("Origin: http://{host}\r\n");
          }
          "missing-type" => kind = "",
          "form-type" => kind = "Content-Type: text/plain\r\n",
          "duplicate-host" => extra = format!("Host: {host}\r\n"),
          "duplicate-origin" => extra = origin.clone(),
          "transfer-encoding" => extra = "Transfer-Encoding: chunked\r\n".into(),
          "oversized-body" => length = "65".into(),
          "truncated-body" => length = "64".into(),
          "bad-body" => payload = "not-json".into(),
          _ => {}
        }
        if mutation == "bad-body" {
          length = payload.len().to_string();
        }
        let length = if mutation == "no-length" {
          String::new()
        } else {
          format!("Content-Length: {length}\r\n")
        };
        let method = if mutation == "get" { "GET" } else { "POST" };
        vec![format!("{method} /api/interval HTTP/1.1\r\nHost: {host}\r\n{origin}{kind}{extra}{length}\r\n{payload}").into_bytes()]
      });
      match mutation {
        "duplicate-host" | "duplicate-origin" | "transfer-encoding" | "oversized-body"
        | "truncated-body" | "no-length" => assert!(response.is_empty(), "{mutation}: {response}"),
        "bad-body" => assert!(response.starts_with("HTTP/1.1 400"), "{response}"),
        "get" => assert!(response.starts_with("HTTP/1.1 503"), "{response}"),
        _ => assert!(response.starts_with("HTTP/1.1 403"), "{mutation}: {response}"),
      }
      assert_eq!(cadence.read().unwrap().value()["requested_ms"], 1000, "{mutation}");
    }
  }

  #[test]
  fn sampling_windows_consume_control_only_at_begin_and_publish_the_completed_interval() {
    let shared = Arc::new(RwLock::new(None));
    let cadence = Arc::new(RwLock::new(Cadence::new(100)));
    let (start_tx, start_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let samples = Arc::clone(&shared);
    let control = Arc::clone(&cadence);
    let sampler = thread::spawn(move || {
      for _ in 0..2 {
        start_rx.recv().unwrap();
        let interval = control.write().unwrap().begin();
        event_tx.send((interval, false, Duration::ZERO)).unwrap();
        let started = Instant::now();
        thread::sleep(Duration::from_millis(interval as u64));
        let time = super::sample_time();
        publish_sample(
          &samples,
          Metrics::default(),
          Vec::new(),
          interval,
          Some(crate::procs::ProcessSnapshot {
            rows: Vec::new(),
            groups: Vec::new(),
            sampled: Instant::now(),
          }),
          time,
        );
        event_tx.send((interval, true, started.elapsed())).unwrap();
      }
    });
    start_tx.send(()).unwrap();
    assert_eq!(event_rx.recv().unwrap().0, 100);
    adjustment(&shared, &cadence, "increase");
    let (interval, done, elapsed) = event_rx.recv().unwrap();
    assert!(done && interval == 100 && elapsed >= Duration::from_millis(100));
    let read = || {
      exchange(&shared, &cadence, |port| {
        vec![format!("GET /api/sample HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").into_bytes()]
      })
    };
    let pending: serde_json::Value = serde_json::from_str(body(&read())).unwrap();
    assert_eq!(pending["interval_ms"], 100);
    assert_eq!(pending["cadence"]["effective_ms"], 100);
    assert_eq!(pending["cadence"]["requested_ms"], 250);
    assert!(pending["processes"].as_array().unwrap().is_empty());
    start_tx.send(()).unwrap();
    assert_eq!(event_rx.recv().unwrap().0, 250);
    let in_flight: serde_json::Value = serde_json::from_str(body(&read())).unwrap();
    assert_eq!(in_flight["sequence"], pending["sequence"]);
    assert_eq!(in_flight["interval_ms"], 100);
    assert_eq!(in_flight["cadence"]["effective_ms"], 250);
    assert_eq!(in_flight["cadence"]["pending"], false);
    assert_eq!(in_flight["processes"], pending["processes"]);
    assert!(
      in_flight["processes_age_ms"].as_u64().unwrap()
        >= pending["processes_age_ms"].as_u64().unwrap()
    );
    adjustment(&shared, &cadence, "increase");
    let (interval, done, elapsed) = event_rx.recv().unwrap();
    assert!(done && interval == 250 && elapsed >= Duration::from_millis(250));
    sampler.join().unwrap();
    let completed: serde_json::Value = serde_json::from_str(body(&read())).unwrap();
    assert_eq!(completed["sequence"], 2);
    assert_eq!(completed["interval_ms"], 250);
    assert_eq!(completed["cadence"]["requested_ms"], 500);
    assert_eq!(completed["cadence"]["effective_ms"], 250);
    assert_eq!(cadence.write().unwrap().begin(), 500);
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
