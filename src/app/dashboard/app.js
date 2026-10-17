"use strict";
const $ = (id) => document.getElementById(id);
const unknown = "\u2014";
const finite = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
const number = (value, digits = 1) => finite(value) ? value.toFixed(digits) : unknown;
const percent = (value) => finite(value) ? `${(Math.min(value, 1) * 100).toFixed(1)}%` : unknown;
const put = (id, value) => { $(id).textContent = value; };
const panelNames = { cpu: "CPU", gpu: "GPU", ram: "memory", power: "power", network: "network", cooling: "temperature and cooling", process: "process" };
const preferenceKey = "macmon.panel-details.v1";
function loadDetails() {
  let saved;
  try { saved = JSON.parse(localStorage.getItem(preferenceKey)); } catch { /* Browser storage is optional. */ }
  return Object.fromEntries(Object.keys(panelNames).map((name) => [name, saved?.[name] === true]));
}
const state = { sample: null, identity: null, received: 0, online: null, waiting: false, busy: false, urgent: false, fans: new Map(), history: [], frame: 0, last: 0, cadence: null, changing: false, intervalError: false, details: loadDetails() };
const motion = $("motion");
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
motion.checked = !reduced.matches;
let pollTimer, staleTimer, controller;
function bytes(value) {
  if (!finite(value)) return unknown;
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  const scale = value > 0 ? Math.max(0, Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1)) : 0;
  return `${(value / 1024 ** scale).toLocaleString("en-US", { maximumSignificantDigits: 3 })} ${units[scale]}`;
}
function watts(value) {
  if (!finite(value)) return "Unavailable";
  const small = value > 0 && value < 1;
  return `${(small ? value * 1000 : value).toLocaleString("en-US", { maximumSignificantDigits: 4 })} ${small ? "mW" : "W"}`;
}
function power(metrics) {
  const read = (key) => !metrics.soc?.vm && finite(metrics[key]) ? metrics[key] : null;
  for (const name of ["cpu", "gpu", "ane", "ram", "gpu_ram"]) put(`${name}-power`, watts(read(`${name}_power`)));
  const reported = read("sys_power"), system = reported > 0 ? reported : null, compute = read("all_power");
  put("system-power", watts(system));
  put("compute-power", watts(compute));
  return [system, compute];
}
function age() { return state.sample ? state.sample.age_ms + performance.now() - state.received : Infinity; }
function limit() { return Math.max(3 * (state.sample?.interval_ms || 1000), 3 * (state.cadence?.effective_ms || 0), 3000); }
function refreshCadence(value = state.cadence) {
  if (!value) return;
  if ([value.requested_ms, value.effective_ms].some((ms) => !Number.isInteger(ms) || ms <= 0)
    || typeof value.pending !== "boolean") throw new Error("Invalid cadence");
  state.cadence = value;
  put("interval-value", `${number(value.effective_ms, 0)} ms`);
  put("interval-feedback", state.changing ? "Applying…" : state.intervalError ? "Change not confirmed"
    : value.pending ? `Pending ${number(value.requested_ms, 0)} ms; finishing current sample` : "Service sampling");
  $("interval-down").disabled = state.changing || value.requested_ms === 250;
  $("interval-up").disabled = state.changing || value.requested_ms === 10000;
  put("cadence-footer", `Local metrics / 60-sample history / ${number(value.effective_ms, 0)} ms sampling`);
}
async function adjustInterval(adjustment) {
  if (state.changing || !state.cadence) return;
  state.changing = true;
  state.intervalError = false;
  refreshCadence();
  const request = new AbortController(), timeout = setTimeout(() => request.abort(), 2500);
  try {
    const response = await fetch("/api/interval", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ adjustment }), signal: request.signal });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    refreshCadence(await response.json());
  } catch {
    state.intervalError = true;
  } finally {
    clearTimeout(timeout);
    state.changing = false;
    refreshCadence();
    clearTimeout(pollTimer);
    state.urgent = true;
    poll();
  }
}
function live() { return state.online && !state.waiting && state.sample && age() <= limit(); }
function moving() { return state.details.cooling && live() && !document.hidden && motion.checked && [...state.fans.values()].some((fan) => fan.rpm > 0); }
function rowDetails(name, limit) {
  const rows = [...$(`${name}-rows`).children], all = state.details[name];
  rows.forEach((row, index) => { row.hidden = !all && index >= limit; });
  put(`${name}-scope`, name === "network"
    ? `${all ? "All" : `Top ${Math.min(limit, rows.length)}`} of ${rows.length} interfaces by current traffic; rates are kept separate.`
    : `${all ? "All" : `Top ${Math.min(limit, rows.length)}`} of ${rows.length} processes in the selected order.`);
}
function applyDetails() {
  for (const [name, label] of Object.entries(panelNames)) {
    const all = state.details[name], button = $(`${name}-toggle`), body = $(`${name}-details`);
    button.setAttribute("aria-expanded", String(all));
    button.setAttribute("aria-label", `${all ? "Less" : "Show"} ${label} details`);
    button.textContent = all ? "Less" : "Details";
    body.dataset.expanded = String(all);
    if (name === "network" || name === "process") {
      body.setAttribute("aria-label", `${name === "network" ? "Network interfaces" : "Processes"}; ${all ? "scroll to see all" : "expand details to see all"}`);
      rowDetails(name, name === "network" ? 3 : 5);
    } else body.hidden = !all;
  }
  drawCharts();
  settle();
}
function changeDetails(names, value) {
  for (const name of names) state.details[name] = value ?? !state.details[name];
  try { localStorage.setItem(preferenceKey, JSON.stringify(state.details)); } catch { /* Keep this view usable without storage. */ }
  applyDetails();
}
function settle() {
  clearTimeout(staleTimer);
  const status = state.waiting ? "waiting" : state.online === null ? "connecting" : !state.online ? "offline" : live() ? "live" : "stale";
  $("status").dataset.state = status;
  put("status", status.toUpperCase());
  const processAge = finite(state.sample?.processes_age_ms) ? state.sample.processes_age_ms + performance.now() - state.received : Infinity;
  const processState = state.waiting ? "WAITING / retained / " : state.online === false ? "OFFLINE / retained / " : processAge > limit() ? "STALE / " : "";
  put("process-status", Array.isArray(state.sample?.processes) ? `${processState}${finite(processAge) ? `${Math.floor(processAge / 1000)}s old` : "Age unavailable"} / CPU 100% = one core` : "Unavailable for this request");
  if (live() && !document.hidden) staleTimer = setTimeout(settle, Math.max(1, limit() - age() + 1));
  if (moving()) {
    if (!state.frame) state.frame = requestAnimationFrame(animate);
  } else {
    cancelAnimationFrame(state.frame);
    state.frame = state.last = 0;
    state.fans.forEach((fan) => { fan.speed = 0; });
  }
}
function animate(now) {
  state.frame = 0;
  if (!moving()) { settle(); return; }
  const delta = state.last ? Math.min((now - state.last) / 1000, 0.05) : 0;
  state.last = now;
  state.fans.forEach((fan) => {
    if (!(fan.rpm > 0)) { fan.speed = 0; return; }
    const target = Math.min(1.5, Math.sqrt(fan.rpm) / 150);
    fan.speed += (target - fan.speed) * (1 - Math.exp(-8 * delta));
    fan.angle = (fan.angle + delta * fan.speed * 360) % 360;
    fan.rotor.style.transform = `rotate(${fan.angle}deg)`;
  });
  state.frame = requestAnimationFrame(animate);
}
function drawCharts() {
  const color = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  ["cpu", "gpu", "ram"].forEach((name, index) => {
    const canvas = $(`${name}-chart`);
    const context = canvas.getContext("2d");
    const width = canvas.clientWidth, height = canvas.clientHeight, scale = window.devicePixelRatio || 1;
    if (!context || !width || !height) return;
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    context.setTransform(scale, 0, 0, scale, 0, 0);
    context.strokeStyle = "#29394c";
    context.lineWidth = 1;
    for (const ratio of [0.25, 0.5, 0.75]) { context.beginPath(); context.moveTo(0, ratio * height); context.lineTo(width, ratio * height); context.stroke(); }
    context.beginPath();
    context.strokeStyle = color;
    context.lineWidth = 1.7;
    let connected = false;
    state.history.forEach((values, point) => {
      const value = values[index];
      if (!finite(value)) { connected = false; return; }
      const x = width * (60 - state.history.length + point) / 59, y = height * (1 - Math.min(value, 1));
      if (connected) context.lineTo(x, y); else context.moveTo(x, y);
      connected = true;
    });
    context.stroke();
  });
  drawPowerChart();
  drawTemperatureChart();
}
function drawPowerChart() {
  const values = state.history.flatMap((point) => point.slice(3, 5)).filter(finite);
  const peak = Math.max(0, ...values), unit = peak > 0 ? 10 ** Math.floor(Math.log10(peak)) : 1;
  const rounded = [1, 2, 5, 10].map((step) => step * unit).find((value) => value >= peak);
  const maximum = finite(rounded) && rounded > 0 ? rounded : peak || 1;
  put("power-range", values.length ? `0–${maximum.toLocaleString("en-US", { maximumSignificantDigits: 3 })} W` : "Scale unavailable");
  const canvas = $("power-chart"), context = canvas.getContext("2d");
  const width = canvas.clientWidth, height = canvas.clientHeight, scale = window.devicePixelRatio || 1;
  if (!context || !width || !height) return;
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.strokeStyle = "#29394c";
  context.lineWidth = 1;
  for (const ratio of [0, 0.5, 1]) { context.beginPath(); context.moveTo(0, ratio * height); context.lineTo(width, ratio * height); context.stroke(); }
  ["--accent", "--power-subtotal"].forEach((color, index) => {
    context.beginPath();
    context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue(color).trim();
    context.lineWidth = 1.8;
    context.setLineDash(index ? [5, 4] : []);
    let connected = false;
    state.history.forEach((point, position) => {
      const value = point[index + 3];
      if (!finite(value)) { connected = false; return; }
      const x = width * (60 - state.history.length + position) / 59, y = height * (1 - value / maximum);
      if (connected) context.lineTo(x, y); else context.moveTo(x, y);
      connected = true;
    });
    context.stroke();
  });
}
const temperatureValue = (value) => typeof value === "number" && Number.isFinite(value);
const celsius = (value) => temperatureValue(value) ? `${value.toFixed(1)} °C` : "Unavailable";
function drawTemperatureChart() {
  const values = state.history.flatMap((point) => point.slice(5, 7)).filter(temperatureValue);
  const low = values.length ? Math.floor(Math.min(...values) / 10) * 10 : 0;
  const high = values.length ? Math.max(low + 10, Math.ceil(Math.max(...values) / 10) * 10) : 100;
  put("temperature-range", values.length ? `${low}–${high} °C / latest ${state.history.length} samples` : "Scale unavailable");
  put("temperature-peaks", ["CPU", "GPU"].map((name, group) => {
    const readings = state.history.map((point) => point[group + 5]).filter(temperatureValue);
    return `${name} ${celsius(readings.length ? Math.max(...readings) : null)}`;
  }).join(" / ") + " / peaks of recorded means, not current sensor hotspots");
  const canvas = $("temperature-chart"), context = canvas.getContext("2d");
  const width = canvas.clientWidth, height = canvas.clientHeight, scale = window.devicePixelRatio || 1;
  if (!context || !width || !height) return;
  canvas.width = Math.round(width * scale); canvas.height = Math.round(height * scale);
  context.setTransform(scale, 0, 0, scale, 0, 0);
  ["--accent", "--power-subtotal"].forEach((color, index) => {
    context.beginPath(); context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue(color).trim();
    context.lineWidth = 1.8; context.setLineDash(index ? [5, 4] : []);
    let connected = false;
    state.history.forEach((point, position) => {
      const value = point[index + 5];
      if (!temperatureValue(value)) { connected = false; return; }
      const x = width * (60 - state.history.length + position) / 59, y = height * (1 - (value - low) / (high - low));
      if (connected) context.lineTo(x, y); else context.moveTo(x, y);
      connected = true;
    });
    context.stroke();
  });
}
function temperatures(sample) {
  const rows = !sample.metrics.soc?.vm && Array.isArray(sample.temperatures) ? sample.temperatures.filter((item) =>
    item && typeof item.name === "string" && item.name && ["cpu", "gpu"].includes(item.group)
      && ["smc", "hid"].includes(item.source) && temperatureValue(item.celsius) && item.celsius > 0 && item.celsius <= 150) : [];
  rows.sort((a, b) => a.group.localeCompare(b.group) || a.name.localeCompare(b.name) || a.source.localeCompare(b.source));
  put("temperature-groups", rows.length ? ["cpu", "gpu"].map((group) => {
    const sensors = rows.filter((item) => item.group === group), values = sensors.map((item) => item.celsius);
    const hottest = sensors.reduce((best, item) => !best || item.celsius > best.celsius ? item : best, null);
    return `${group.toUpperCase()}: ${values.length ? `${values.length} sensors / current min ${celsius(Math.min(...values))} / max ${celsius(hottest.celsius)} / hottest ${hottest.name}` : "Unavailable"}`;
  }).join(" · ") : "Sensor detail unavailable; the backend may not expose individual readings.");
  $("temperature-table").hidden = !rows.length;
  $("temperature-rows").replaceChildren(...rows.map((item) => {
    const row = document.createElement("tr");
    [item.name, item.group.toUpperCase(), item.source.toUpperCase(), celsius(item.celsius)].forEach((value, index) => {
      const cell = document.createElement("td"); cell.textContent = value;
      cell.dataset.label = ["Sensor", "Group", "Source", "°C"][index]; row.append(cell);
    });
    return row;
  }));
}
function network(metrics) {
  const rows = Array.isArray(metrics.network) ? metrics.network.filter((item) => item && typeof item.name === "string") : [];
  const traffic = (item) => [item.rx_bytes_per_sec, item.tx_bytes_per_sec].reduce((sum, rate) => sum + (finite(rate) ? rate : 0), 0);
  rows.sort((a, b) => traffic(b) - traffic(a) || a.name.localeCompare(b.name));
  $("network-table").hidden = !rows.length;
  $("network-empty").hidden = !!rows.length;
  put("network-empty", Array.isArray(metrics.network) ? "No active interfaces reported." : "Network telemetry unavailable.");
  put("network-count", `${rows.length} INTERFACES`);
  $("network-rows").replaceChildren(...rows.map((item) => {
    const row = document.createElement("tr");
    [item.name, bytes(item.rx_bytes_per_sec), bytes(item.tx_bytes_per_sec), bytes(item.rx_bytes), bytes(item.tx_bytes)].forEach((value, index) => {
      const cell = document.createElement("td");
      cell.textContent = value;
      cell.dataset.label = ["Interface", "RX / s", "TX / s", "RX total", "TX total"][index];
      row.append(cell);
    });
    return row;
  }));
  rowDetails("network", 3);
}
function cooling(metrics) {
  const next = new Map();
  const items = !metrics.soc?.vm && Array.isArray(metrics.fans) ? metrics.fans : [];
  items.filter((item) => item && typeof item.name === "string" && item.name).forEach((item) => {
    let fan = state.fans.get(item.name);
    if (!fan) {
      const node = $("fan-template").content.firstElementChild.cloneNode(true);
      const rotor = node.querySelector(".rotor");
      for (let blade = 0; blade < 7; blade++) {
        const leaf = document.createElement("i");
        leaf.className = "blade";
        leaf.style.transform = `rotate(${blade * 360 / 7}deg)`;
        rotor.append(leaf);
      }
      fan = { node, rotor, angle: 0, speed: 0 };
    }
    fan.rpm = finite(item.rpm) ? item.rpm : null;
    if (!(fan.rpm > 0)) fan.speed = 0;
    fan.node.querySelector(".fan-name").textContent = item.name;
    fan.node.querySelector(".fan-rpm").textContent = `${number(fan.rpm, 0)} RPM`;
    fan.node.querySelector(".fan-limit").textContent = finite(item.max_rpm) ? `Reported max ${number(item.max_rpm, 0)} RPM` : "Maximum not reported";
    next.set(item.name, fan);
  });
  state.fans = next;
  $("fans").replaceChildren(...[...next.values()].map((fan) => fan.node));
  $("fans").hidden = !next.size;
  $("fan-empty").hidden = !!next.size;
  put("fan-empty", metrics.soc?.vm ? "Fan telemetry unavailable in a virtual machine." : "No fan telemetry reported.");
  put("fan-summary", next.size ? [...next.values()].map((fan) => `${fan.node.querySelector(".fan-name").textContent} ${number(fan.rpm, 0)} RPM`).join(" / ") : $("fan-empty").textContent);
}
function processes() {
  const items = state.sample?.processes;
  const rows = Array.isArray(items) ? items.filter((row) => row && typeof row.name === "string" && Number.isSafeInteger(row.pid) && row.pid > 0) : [];
  const key = $("process-sort").value || "cpu_pct";
  rows.sort((a, b) => Number(finite(b[key])) - Number(finite(a[key])) || (finite(a[key]) ? b[key] - a[key] : 0) || a.pid - b.pid);
  $("process-table").hidden = !rows.length;
  $("process-empty").hidden = !!rows.length;
  put("process-empty", Array.isArray(items) ? "No processes reported." : "Process telemetry requires a local loopback request.");
  put("process-count", `${rows.length} PROCESSES`);
  $("process-rows").replaceChildren(...rows.map((item) => {
    const row = document.createElement("tr");
    [item.name, finite(item.cpu_pct) ? `${number(item.cpu_pct)}%` : unknown, finite(item.gpu_pct) ? `${number(item.gpu_pct)}%` : unknown, bytes(item.mem_bytes)].forEach((value, index) => {
      const cell = document.createElement("td");
      cell.textContent = value;
      cell.dataset.label = ["Process / PID", "CPU %", "GPU %", "Memory"][index];
      if (!index) { const pid = document.createElement("small"); pid.textContent = `PID ${item.pid}`; cell.append(pid); }
      row.append(cell);
    });
    return row;
  }));
  rowDetails("process", 5);
}
function update(sample) {
  const metrics = sample.metrics, vm = metrics.soc?.vm, memory = metrics.memory || {};
  const frequencies = metrics.soc?.gpu_freqs;
  const gpuUnavailable = vm || (Array.isArray(frequencies) && !frequencies.some((frequency, index) => index > 0 && finite(frequency) && frequency > 0));
  const gpu = gpuUnavailable ? null : metrics.gpu_active_ratio;
  const ram = finite(memory.ram_usage) && finite(memory.ram_total) && memory.ram_total > 0 ? memory.ram_usage / memory.ram_total : null;
  put("cpu-value", percent(metrics.cpu_active_ratio));
  put("gpu-value", percent(gpu));
  put("ram-value", percent(ram));
  const tiers = vm ? unknown : (Array.isArray(metrics.cpu_tiers) ? metrics.cpu_tiers : []).map((tier) => `${tier.label} ${number(tier.freq_mhz, 0)} MHz`).join(" / ") || unknown;
  for (const name of ["cpu", "gpu"]) {
    const value = vm ? null : metrics.temp?.[`${name}_temp_avg`];
    put(`${name}-temp`, celsius(value));
    put(`cooling-${name}-temp`, celsius(value));
  }
  put("cpu-detail", `${watts(vm ? null : metrics.cpu_power)} / ${tiers}`);
  put("gpu-detail", `${watts(vm ? null : metrics.gpu_power)} / ${number(gpuUnavailable ? null : metrics.gpu_freq_mhz, 0)} MHz`);
  put("ram-detail", `${bytes(memory.ram_usage)} used / ${bytes(memory.ram_total)} total / ${bytes(memory.swap_usage)} swap`);
  put("ram-summary", `${bytes(memory.ram_usage)} / ${bytes(memory.ram_total)}`);
  put("device", [metrics.soc?.chip_name || "Local device", metrics.soc?.mac_model, vm ? "Virtual machine" : ""].filter(Boolean).join(" / "));
  put("sample-time", `Sample ${new Date(sample.sampled_at).toLocaleTimeString("en-US", { hour12: false })}`);
  state.history.push([metrics.cpu_active_ratio, gpu, ram, ...power(metrics), ...["cpu", "gpu"].map((name) => vm ? null : metrics.temp?.[`${name}_temp_avg`])]);
  if (state.history.length > 60) state.history.shift();
  network(metrics);
  cooling(metrics);
  temperatures(sample);
  processes();
  drawCharts();
}
async function poll() {
  if (document.hidden || state.busy) return;
  state.busy = true;
  state.urgent = false;
  const started = performance.now(), request = new AbortController();
  controller = request;
  const timeout = setTimeout(() => request.abort(), 2500);
  try {
    const response = await fetch("/api/sample", { cache: "no-store", signal: request.signal });
    if (response.status === 503) {
      state.waiting = true;
      state.online = true;
      settle();
      return;
    }
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const sample = await response.json();
    if (!Number.isSafeInteger(sample.sequence) || sample.sequence < 0 || !finite(sample.age_ms)
      || !finite(sample.interval_ms) || !sample.interval_ms || !sample.metrics
      || typeof sample.metrics !== "object" || Array.isArray(sample.metrics)
      || !Number.isFinite(Date.parse(sample.sampled_at))) throw new Error("Invalid sample");
    state.sample = sample;
    if (sample.cadence) refreshCadence(sample.cadence);
    // Count the full request duration as a conservative bound for response transit time.
    state.received = started;
    state.online = true;
    state.waiting = false;
    const identity = `${sample.sequence}/${sample.sampled_at}`;
    if (identity !== state.identity) {
      update(sample);
      state.identity = identity;
    }
    settle();
  } catch (error) {
    if (request.signal.reason !== "hidden") {
      state.waiting = false;
      state.online = false;
      settle();
    }
  } finally {
    clearTimeout(timeout);
    controller = null;
    state.busy = false;
    const delay = state.cadence ? Math.min(state.cadence.effective_ms, state.cadence.pending ? 1000 : 10000) : 1000;
    if (!document.hidden) pollTimer = setTimeout(poll, state.urgent ? 0 : Math.max(0, Math.max(250, delay) - (performance.now() - started)));
  }
}
motion.addEventListener("change", settle);
$("process-sort").addEventListener("change", processes);
Object.keys(panelNames).forEach((name) => $(`${name}-toggle`).addEventListener("click", () => changeDetails([name])));
$("expand-all").addEventListener("click", () => changeDetails(Object.keys(panelNames), true));
$("collapse-all").addEventListener("click", () => changeDetails(Object.keys(panelNames), false));

$("interval-down").addEventListener("click", () => adjustInterval("decrease"));
$("interval-up").addEventListener("click", () => adjustInterval("increase"));
reduced.addEventListener("change", () => { if (reduced.matches) { motion.checked = false; settle(); } });
document.addEventListener("visibilitychange", () => {
  clearTimeout(pollTimer);
  if (document.hidden) controller?.abort("hidden"); else { state.urgent = true; poll(); }
  settle();
});
new ResizeObserver(drawCharts).observe($("cpu-chart").parentElement.parentElement);
applyDetails();
poll();
