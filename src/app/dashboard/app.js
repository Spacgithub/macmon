"use strict";
const $ = (id) => document.getElementById(id);
const unknown = "\u2014";
const finite = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
const ratio = (value) => finite(value) && value <= 1 ? value : null;
const residency = (value) => ratio(value) === null ? "Unavailable" : percent(value);
const frequency = (value) => Number.isSafeInteger(value) && value > 0 && value <= 4294967295 ? value : null;
const number = (value, digits = 1) => finite(value) ? value.toFixed(digits) : unknown;
const percent = (value) => finite(value) ? `${(Math.min(value, 1) * 100).toFixed(1)}%` : unknown;
const put = (id, value) => { $(id).textContent = value; };
const panelNames = { cpu: "CPU", gpu: "GPU", ram: "memory", power: "power", disk: "disk", network: "network", cooling: "temperature and cooling", process: "process" };
const preferenceKey = "macmon.panel-details.v1";
function loadDetails() {
  let saved;
  try { saved = JSON.parse(localStorage.getItem(preferenceKey)); } catch { /* Browser storage is optional. */ }
  return Object.fromEntries(Object.keys(panelNames).map((name) => [name, saved?.[name] === true]));
}
const state = { sample: null, identity: null, received: 0, online: null, waiting: false, busy: false, urgent: false, fans: new Map(), history: [], disks: new Map(), diskHistory: new Map(), diskSelected: null, cpuTierRows: new Map(), cpuCoreRows: new Map(), networkRows: new Map(), networkStatus: "Waiting for rates", frame: 0, last: 0, cadence: null, changing: false, intervalError: false, details: loadDetails() };
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
function memoryActivity(sample) {
  const activity = sample.memory_activity;
  const level = ["normal", "warning", "critical"].includes(activity?.pressure) ? activity.pressure : "unavailable";
  $("memory-pressure").dataset.level = level;
  put("memory-pressure", level === "unavailable" ? "Unavailable" : level[0].toUpperCase() + level.slice(1));
  put("swap-in-rate", finite(activity?.swap_in_bytes_per_sec) ? `${bytes(activity.swap_in_bytes_per_sec)}/s` : "Unavailable");
  put("swap-out-rate", finite(activity?.swap_out_bytes_per_sec) ? `${bytes(activity.swap_out_bytes_per_sec)}/s` : "Unavailable");
}
function measuredRows(id, cache, items, headings) {
  const next = new Map(), nodes = items.map(([key, values]) => {
    const row = cache.get(key) || document.createElement("tr");
    if (!row.children.length) headings.forEach((heading) => { const cell = document.createElement("td"); cell.dataset.label = heading; row.append(cell); });
    values.forEach((value, index) => { if (row.children[index].textContent !== value) row.children[index].textContent = value; });
    next.set(key, row); return row;
  });
  diskChildren($(id), nodes);
  return next;
}
function computeDetails(metrics, gpuUnavailable) {
  const soc = metrics.soc || {}, raw = Array.isArray(metrics.cpu_tiers) ? metrics.cpu_tiers : [], info = Array.isArray(soc.cpu_tiers) ? soc.cpu_tiers : [];
  const tiers = raw.flatMap((tier, index) => {
    const label = diskText(tier?.label, 32);
    if (!label) return [];
    const metadata = info[index]?.label === label ? info[index] : null, installed = diskInteger(metadata?.cores) > 0 && metadata.cores <= 255 ? metadata.cores : null;
    const seen = new Set(), reported = Array.isArray(tier.cores) ? tier.cores : [];
    const cores = reported.filter((core) => {
      if (!core || diskInteger(core.die_id) === null || diskInteger(core.core_id) === null) return false;
      const key = `${core.die_id}/${core.core_id}`;
      if (seen.has(key)) return false;
      seen.add(key); return true;
    }).slice().sort((a, b) => a.die_id - b.die_id || a.core_id - b.core_id);
    const complete = installed > 0 && cores.length === installed && cores.length === reported.length
      && Array.isArray(metadata?.freqs) && metadata.freqs.length > 0 && metadata.freqs.every((value) => frequency(value) !== null)
      && cores.every((core) => ratio(core.active_ratio) !== null && ratio(core.scaled_ratio) !== null && (frequency(core.freq_mhz) !== null || (core.freq_mhz === 0 && core.active_ratio === 0)));
    return [{ tier, label, index, installed, cores, complete }];
  });
  // A single-tier tick fallback can retain a positive, floored tier MHz even with a SoC table.
  const weighted = !soc.vm && tiers.length > 0 && tiers.length === raw.length && tiers.length === info.length
    && tiers.every((item) => item.complete && ratio(item.tier.active_ratio) !== null && ratio(item.tier.scaled_ratio) !== null)
    && (tiers.length > 1 || tiers.some((item) => item.cores.some((core) => frequency(core.freq_mhz) !== null)));
  const total = tiers.reduce((sum, item) => sum + item.cores.length, 0), installed = tiers.length > 0 && tiers.length === info.length && tiers.every((item) => item.installed !== null) ? tiers.reduce((sum, item) => sum + item.installed, 0) : null;
  put("cpu-core-count", `${total} reported / ${installed === null ? "unknown" : installed} installed cores`);
  put("cpu-reported", residency(metrics.cpu_active_ratio));
  put("cpu-weighted", residency(weighted ? metrics.cpu_scaled_ratio : null));
  put("cpu-package-power", watts(soc.vm ? null : metrics.cpu_power));
  const tierFrequency = (item) => weighted && item.cores.some((core) => frequency(core.freq_mhz) !== null) ? frequency(item.tier.freq_mhz) : null;
  state.cpuTierRows = measuredRows("cpu-tier-rows", state.cpuTierRows, tiers.map((item) => [`${item.index}/${item.label}`, [item.label,
    `${item.cores.length} / ${item.installed === null ? "unknown" : item.installed}`, residency(item.cores.length ? item.tier.active_ratio : null), residency(weighted ? item.tier.scaled_ratio : null), tierFrequency(item) === null ? "Not reported" : `${tierFrequency(item)} MHz`]]),
  ["Type", "Reported / installed cores", "Activity", "Weighted", "Aggregate MHz"]);
  state.cpuCoreRows = measuredRows("cpu-core-rows", state.cpuCoreRows, tiers.flatMap((item) => item.cores.map((core) => [`${item.index}/${item.label}/${core.die_id}/${core.core_id}`,
    [item.label, String(core.die_id), String(core.core_id), residency(core.active_ratio), residency(weighted ? core.scaled_ratio : null), weighted && frequency(core.freq_mhz) !== null ? `${core.freq_mhz} MHz` : "Not reported"]])),
  ["Type", "Die", "Core", "Activity", "Weighted", "Active MHz"]);
  $("cpu-tier-table").hidden = !tiers.length; $("cpu-core-table").hidden = !total; $("cpu-core-empty").hidden = !!total;
  put("cpu-detail", `${watts(soc.vm ? null : metrics.cpu_power)} / ${tiers.map((item) => `${item.label} ${tierFrequency(item) === null ? "frequency unavailable" : `${tierFrequency(item)} MHz`}`).join(" / ") || "Tier telemetry unavailable"}`);
  const frequencies = !soc.vm && Array.isArray(soc.gpu_freqs) && soc.gpu_freqs.length > 1 && soc.gpu_freqs.every((value) => value === 0 || frequency(value) !== null) ? soc.gpu_freqs.slice(1).filter((value) => frequency(value) !== null) : [];
  const gpuFrequency = !gpuUnavailable && frequencies.length ? frequency(metrics.gpu_freq_mhz) : null, gpuWeighted = gpuFrequency === null ? null : ratio(metrics.gpu_scaled_ratio);
  put("gpu-reported", residency(gpuUnavailable ? null : metrics.gpu_active_ratio)); put("gpu-weighted", residency(gpuWeighted));
  put("gpu-frequency", gpuFrequency === null ? "Not reported" : `${gpuFrequency} MHz`);
  put("gpu-range", frequencies.length ? `${frequencies.reduce((min, value) => Math.min(min, value))}–${frequencies.reduce((max, value) => Math.max(max, value))} MHz` : "Unavailable");
  put("gpu-cores", !soc.vm && diskInteger(soc.gpu_cores) > 0 && soc.gpu_cores <= 255 ? String(soc.gpu_cores) : "Unavailable");
  put("gpu-package-power", watts(soc.vm ? null : metrics.gpu_power));
  put("gpu-detail", `${watts(soc.vm ? null : metrics.gpu_power)} / ${gpuFrequency === null ? "Active frequency not reported" : `${gpuFrequency} MHz`}`);
  return [weighted ? ratio(metrics.cpu_scaled_ratio) : null, gpuWeighted];
}
const diskInteger = (value) => Number.isSafeInteger(value) && value >= 0 ? value : null;
const diskByte = (value, maximum = 255) => diskInteger(value) !== null && value <= maximum ? value : null;
const diskText = (value, maximum = 160) => typeof value === "string" && value.trim() && value.length <= maximum && !/[\u0000-\u001f\u007f-\u009f]/.test(value) ? value : null;
const u128Max = (1n << 128n) - 1n;
function diskU128(value) {
  if (typeof value !== "string" || value.length > 39 || !/^(0|[1-9][0-9]{0,38})$/.test(value)) return null;
  const integer = BigInt(value);
  return integer <= u128Max ? integer : null;
}
function diskLifetime(value) {
  const units = diskU128(value);
  if (units === null || units > u128Max / 512000n) return "Unavailable";
  const total = units * 512000n, names = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
  let scale = 0, divisor = 1n;
  while (scale < names.length - 1 && total >= divisor * 1024n) { divisor *= 1024n; scale++; }
  return `${(total / divisor).toLocaleString("en-US")}${scale ? `.${total % divisor * 10n / divisor}` : ""} ${names[scale]}`;
}
function diskBytes(value) {
  if (!finite(value)) return "Unavailable";
  if (value === 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let scale = 0;
  while (scale < units.length - 1 && value >= 1024) { value /= 1024; scale++; }
  return `${value >= 10000 ? value.toExponential(2) : value.toLocaleString("en-US", { maximumSignificantDigits: 3 })} ${units[scale]}`;
}
const diskRate = (value) => finite(value) ? `${diskBytes(value)}/s` : "Unavailable";
const diskCount = (value) => diskInteger(value) === null ? "Unavailable" : value === 0 ? "0" : value.toLocaleString("en-US");
const diskIOPS = (value) => finite(value) ? `${value === 0 ? "0" : value >= 10000 ? value.toExponential(2) : value.toLocaleString("en-US", { maximumSignificantDigits: 3 })} IOPS` : "Unavailable";
const diskUsed = (value) => value === 255 ? ">254% (raw 255)" : value === null ? "Unavailable" : `${value}%`;
function diskHealth(raw) {
  const status = ["available", "unsupported", "unavailable"].includes(raw?.state) ? raw.state : "unavailable";
  const value = status === "available" && raw?.nvme && typeof raw.nvme === "object" && !Array.isArray(raw.nvme) ? raw.nvme : null;
  return { status, age: diskInteger(raw?.age_ms), value };
}
function diskHealthView(health) {
  const elapsed = performance.now() - state.received;
  const age = health.age === null ? Infinity : health.age + (state.sample?.age_ms ?? Infinity) + elapsed;
  const knownAge = Number.isFinite(age) && age >= 0 && age <= Number.MAX_SAFE_INTEGER;
  const warning = diskByte(health.value?.critical_warning), used = diskByte(health.value?.percentage_used);
  const mode = health.status !== "available" || !health.value ? health.status === "unsupported" ? "unsupported" : "unavailable"
    : !knownAge || warning === null ? "unavailable" : age > 120000 ? "stale" : !live() ? "retained" : "current";
  const label = mode === "unsupported" ? "Unsupported" : mode === "unavailable" ? "Unavailable"
    : `${mode === "current" ? "" : `${mode === "stale" ? "Stale" : "Retained"} / `}${warning === null ? "Warning unavailable" : warning ? `Warning 0x${warning.toString(16).padStart(2, "0")}` : "No NVMe warning"}`;
  return { mode, age, label: `${label}${health.value && used !== null ? ` / used ${diskUsed(used)}` : ""}`,
    ageLabel: knownAge ? `${Math.floor(age / 1000)}s old / ${mode}` : "Age unavailable", warning };
}
function diskField(parent, label) {
  const group = document.createElement("div"), term = document.createElement("dt"), value = document.createElement("dd");
  term.textContent = label; group.append(term, value); parent.append(group); return value;
}
function diskNode() {
  const summary = document.createElement("article"), detail = document.createElement("article");
  const option = document.createElement("option");
  summary.className = "disk-summary-card"; detail.className = "disk-device";
  const title = document.createElement("h3"), detailTitle = document.createElement("h3"), badge = document.createElement("p"), detailBadge = document.createElement("p");
  badge.className = detailBadge.className = "disk-health";
  const rates = document.createElement("dl"), detailRates = document.createElement("dl"), data = document.createElement("dl");
  rates.className = detailRates.className = "disk-rates"; data.className = "disk-data";
  summary.append(title, badge, rates); detail.append(detailTitle, detailBadge, detailRates, data);
  const compact = [rates, detailRates].map((parent) => ["Read", "Write", "Read IOPS", "Write IOPS"].map((label) => diskField(parent, label)));
  const labels = ["Device", "Driver read", "Driver write", "Read operations", "Write operations", "Health age", "Critical warning", "Endurance used", "Available spare", "Spare threshold", "Temperature", "Lifetime host read", "Lifetime host write", "Power cycles", "Power-on hours", "Unsafe shutdowns", "Media errors", "Error log entries"];
  return { summary, detail, option, title, detailTitle, badge, detailBadge, compact, fields: labels.map((label) => diskField(data, label)) };
}
function diskChildren(parent, children) {
  if (parent.children.length === children.length && [...parent.children].every((child, index) => child === children[index])) return false;
  parent.replaceChildren(...children);
  return true;
}
function diskScope() {
  const all = state.details.disk, count = state.disks.size;
  $("disk-summary").hidden = all;
  [...$("disk-summary").children].forEach((row, index) => { row.hidden = index >= 3; });
  put("disk-scope", state.diskAvailable ? `${all ? "All" : `Top ${Math.min(3, count)}`} of ${count} physical ${count === 1 ? "device" : "devices"} by current I/O; each disk stays separate.` : "Physical disk readings unavailable.");
}
function refreshDiskHealth() {
  let next = Infinity;
  for (const view of state.disks.values()) {
    const health = diskHealthView(view.health), raw = view.health.value;
    for (const badge of [view.badge, view.detailBadge]) { badge.textContent = health.label; badge.dataset.state = health.mode; badge.dataset.warning = String(health.warning !== null && health.warning > 0); }
    const percentValue = (value) => value === null ? "Unavailable" : `${value}%`;
    const integer = (value) => { const parsed = diskU128(value); return parsed === null ? "Unavailable" : parsed.toLocaleString("en-US"); };
    const temperature = raw?.temperature_celsius;
    const warning = health.warning === null ? "Unavailable" : `0x${health.warning.toString(16).padStart(2, "0")}${health.warning ? " / reported warning" : " / none reported"}`;
    const values = [health.ageLabel, warning, diskUsed(diskByte(raw?.percentage_used)), percentValue(diskByte(raw?.available_spare_percent, 100)), percentValue(diskByte(raw?.spare_threshold_percent, 100)),
      typeof temperature === "number" && Number.isFinite(temperature) && temperature >= -273.15 && temperature <= 65261.85 ? `${temperature.toFixed(1)} °C` : "Unavailable",
      diskLifetime(raw?.data_units_read), diskLifetime(raw?.data_units_written), ...["power_cycles", "power_on_hours", "unsafe_shutdowns", "media_errors", "error_log_entries"].map((key) => integer(raw?.[key]))];
    values.forEach((value, index) => { view.fields[index + 5].textContent = value; });
    if (health.mode === "current" || health.mode === "retained") next = Math.min(next, Math.max(1, 120001 - health.age));
  }
  return next;
}
function disks(sample) {
  const available = Array.isArray(sample.disks), next = new Map();
  const rows = available ? sample.disks.filter((item) => item && typeof item.id === "string" && /^[A-Za-z0-9:._/-]{1,128}$/.test(item.id) && diskText(item.name)) : [];
  const traffic = (item) => (finite(item.read_bytes_per_sec) ? item.read_bytes_per_sec / 2 : 0) + (finite(item.write_bytes_per_sec) ? item.write_bytes_per_sec / 2 : 0);
  rows.sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
  for (const item of rows) {
    if (next.has(item.id)) continue;
    const view = state.disks.get(item.id) || diskNode();
    view.health = diskHealth(item.health); view.name = item.name; view.traffic = traffic(item);
    if (view.option.value !== item.id) view.option.value = item.id;
    if (view.option.textContent !== item.name) view.option.textContent = item.name;
    view.title.textContent = view.detailTitle.textContent = item.name;
    [diskRate(item.read_bytes_per_sec), diskRate(item.write_bytes_per_sec), diskIOPS(item.read_iops), diskIOPS(item.write_iops)].forEach((value, index) => { view.compact.forEach((group) => { group[index].textContent = value; }); });
    const metadata = [diskText(item.model), typeof item.internal === "boolean" ? item.internal ? "Internal" : "External" : null,
      diskInteger(item.size_bytes) === null ? null : diskBytes(item.size_bytes)].filter((value) => value !== null).join(" / ") || "Unavailable";
    [metadata, diskBytes(diskInteger(item.read_bytes)), diskBytes(diskInteger(item.write_bytes)), diskCount(item.read_operations), diskCount(item.write_operations)].forEach((value, index) => { view.fields[index].textContent = value; });
    const history = state.diskHistory.get(item.id) || [];
    history.push([item.read_bytes_per_sec, item.write_bytes_per_sec].map((value) => finite(value) ? value : null));
    if (history.length > 60) history.shift();
    state.diskHistory.set(item.id, history); next.set(item.id, view);
  }
  state.disks = next;
  state.diskAvailable = available && (!sample.disks.length || next.size > 0);
  for (const id of state.diskHistory.keys()) if (!next.has(id)) state.diskHistory.delete(id);
  if (!next.has(state.diskSelected)) state.diskSelected = next.keys().next().value ?? null;
  const select = $("disk-select");
  diskChildren(select, [...next.values()].map((view) => view.option));
  if (select.value !== (state.diskSelected || "")) select.value = state.diskSelected || "";
  if (select.disabled !== !next.size) select.disabled = !next.size;
  const ranked = [...next.values()].sort((a, b) => b.traffic - a.traffic || a.name.localeCompare(b.name));
  diskChildren($("disk-summary"), ranked.map((view) => view.summary));
  const scroll = $("disk-scroll"), top = scroll.scrollTop;
  if (diskChildren($("disk-rows"), [...next.values()].map((view) => view.detail))) scroll.scrollTop = top;
  put("disk-count", state.diskAvailable ? `${next.size} PHYSICAL ${next.size === 1 ? "DEVICE" : "DEVICES"}` : "DISK TELEMETRY UNAVAILABLE");
  $("disk-empty").hidden = !!next.size;
  put("disk-empty", available && !sample.disks.length ? "No physical disks reported." : "Disk telemetry unavailable.");
  diskScope(); refreshDiskHealth();
}
function drawDiskChart() {
  const history = state.diskHistory.get(state.diskSelected) || [], name = state.disks.get(state.diskSelected)?.name;
  const values = history.flat().filter(finite), maximum = Math.max(1, ...values);
  put("disk-history-label", name ? `${name} / latest ${history.length} samples / separate read and write lines` : "Select a reported disk");
  put("disk-range", values.length ? `0–${diskRate(maximum)}` : "Scale unavailable");
  const canvas = $("disk-chart"), context = canvas.getContext("2d"), width = canvas.clientWidth, height = canvas.clientHeight, scale = window.devicePixelRatio || 1;
  if (!context || !width || !height) return;
  canvas.width = Math.round(width * scale); canvas.height = Math.round(height * scale); context.setTransform(scale, 0, 0, scale, 0, 0);
  ["--accent", "--power-subtotal"].forEach((color, index) => {
    context.beginPath(); context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue(color).trim(); context.lineWidth = 1.8; context.setLineDash(index ? [5, 4] : []);
    let connected = false;
    history.forEach((point, position) => {
      const value = point[index];
      if (!finite(value)) { connected = false; return; }
      const x = width * (60 - history.length + position) / 59, y = height * (1 - value / maximum);
      if (connected) context.lineTo(x, y); else context.moveTo(x, y);
      connected = true;
    }); context.stroke();
  });
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
  put(`${name}-scope`, `${all ? "All" : `Top ${Math.min(limit, rows.length)}`} of ${rows.length} processes in the selected order.`);
}
function applyDetails() {
  for (const [name, label] of Object.entries(panelNames)) {
    const all = state.details[name], button = $(`${name}-toggle`), body = $(`${name}-details`);
    button.setAttribute("aria-expanded", String(all));
    button.setAttribute("aria-label", `${all ? "Less" : "Show"} ${label} details`);
    button.textContent = all ? "Less" : "Details";
    body.dataset.expanded = String(all);
    if (name === "process") {
      body.setAttribute("aria-label", `Processes; ${all ? "scroll to see all" : "expand details to see all"}`);
      rowDetails(name, 5);
    } else body.hidden = !all;
  }
  $("compute-details").hidden = !["cpu", "gpu", "ram"].some((name) => state.details[name]);
  $("detail-grid").dataset.expanded = String(state.details.network || state.details.cooling);
  diskScope();
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
  put("network-state", `${state.sample && !live() ? "Retained / " : ""}${state.networkStatus}`);
  const processAge = finite(state.sample?.processes_age_ms) ? state.sample.processes_age_ms + performance.now() - state.received : Infinity;
  const processState = state.waiting ? "WAITING / retained / " : state.online === false ? "OFFLINE / retained / " : processAge > limit() ? "STALE / " : "";
  put("process-status", Array.isArray(state.sample?.processes) ? `${processState}${finite(processAge) ? `${Math.floor(processAge / 1000)}s old` : "Age unavailable"} / CPU 100% = one core` : "Unavailable for this request");
  const diskDelay = refreshDiskHealth();
  if (!document.hidden) {
    const delay = Math.min(live() ? Math.max(1, limit() - age() + 1) : Infinity, diskDelay);
    if (Number.isFinite(delay)) staleTimer = setTimeout(settle, delay);
  }
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
  ["cpu", "gpu", "ram"].forEach((name, index) => {
    if (name !== "ram") {
      const summary = (column) => {
        const values = state.history.map((point) => point[column]).filter((value) => ratio(value) !== null);
        return values.length ? `${values.length} valid / mean ${residency(values.reduce((sum, value) => sum + value, 0) / values.length)} / peak ${residency(Math.max(...values))}` : "Unavailable";
      };
      put(`${name}-history`, `0–100% / ${state.history.length}/60 samples · Activity ${summary(index)} · Weighted ${summary(index + 7)}`);
    }
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
    (name === "ram" ? [index] : [index, index + 7]).forEach((column, series) => {
      context.beginPath(); context.setLineDash(series ? [5, 4] : []);
      context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue(series ? "--power-subtotal" : "--accent").trim();
      context.lineWidth = 1.7;
      let connected = false;
      state.history.forEach((values, point) => {
        const value = values[column];
        if (ratio(value) === null) { connected = false; return; }
        const x = width * (60 - state.history.length + point) / 59, y = height * (1 - value);
        if (connected) context.lineTo(x, y); else context.moveTo(x, y);
        connected = true;
      });
      context.stroke();
    });
    context.setLineDash([]);
  });
  drawPowerChart();
  drawTemperatureChart();
  drawDiskChart();
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
  const seen = new Set(), rows = Array.isArray(metrics.network) ? metrics.network.filter((item) => {
    if (!diskText(item?.name, 160) || seen.has(item.name)) return false;
    seen.add(item.name); return true;
  }).slice().sort((a, b) => a.name.localeCompare(b.name)) : [];
  const rates = (item) => [item.rx_bytes_per_sec, item.tx_bytes_per_sec], traffic = (item) => rates(item).reduce((sum, rate) => sum + (finite(rate) ? rate / 2 : 0), 0);
  const selected = rows.slice().sort((a, b) => Number(rates(b).some(finite)) - Number(rates(a).some(finite)) || traffic(b) - traffic(a) || a.name.localeCompare(b.name))[0];
  const available = Array.isArray(metrics.network) && (!metrics.network.length || rows.length > 0);
  const active = rows.filter((item) => rates(item).some((value) => finite(value) && value > 0)).length, pending = rows.filter((item) => !rates(item).every(finite)).length;
  const rate = (value) => finite(value) ? `${diskBytes(value)}/s` : value == null ? "Pending" : "Unavailable";
  $("network-table").hidden = !rows.length;
  $("network-empty").hidden = !!rows.length;
  put("network-empty", available ? "No interfaces reported." : "Network telemetry unavailable.");
  put("network-count", available ? `${rows.length} OBSERVED / ${active} TRAFFIC / ${pending} PENDING` : "INTERFACE TELEMETRY UNAVAILABLE");
  put("network-observer", selected ? `Auto / ${selected.name}` : available ? "Auto / no interface" : "Auto / unavailable");
  put("network-rx", selected ? rate(selected.rx_bytes_per_sec) : "Unavailable"); put("network-tx", selected ? rate(selected.tx_bytes_per_sec) : "Unavailable");
  state.networkStatus = !available ? "Collection unavailable" : !rows.length ? "No observation interface" : active ? "Observed traffic" : pending ? "Rates pending" : "Measured zero rates";
  put("network-scope", "Auto selects one busiest observed interface; rates are kept separate. Traffic and pending counts may overlap. This does not establish Internet connectivity.");
  state.networkRows = measuredRows("network-rows", state.networkRows, rows.map((item) => [item.name, [item.name, rate(item.rx_bytes_per_sec), rate(item.tx_bytes_per_sec), diskBytes(diskInteger(item.rx_bytes)), diskBytes(diskInteger(item.tx_bytes))]]), ["Interface", "RX / s", "TX / s", "RX total", "TX total"]);
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
  put("cpu-value", percent(ratio(metrics.cpu_active_ratio)));
  put("gpu-value", percent(ratio(gpu)));
  put("ram-value", percent(ram));
  for (const name of ["cpu", "gpu"]) {
    const value = vm ? null : metrics.temp?.[`${name}_temp_avg`];
    put(`${name}-temp`, celsius(value));
    put(`cooling-${name}-temp`, celsius(value));
  }
  const weighted = computeDetails(metrics, gpuUnavailable);
  put("ram-detail", `${bytes(memory.ram_usage)} used / ${bytes(memory.ram_total)} total / Swap ${bytes(memory.swap_usage)} used / ${bytes(memory.swap_total)} allocated`);
  put("ram-summary", `${bytes(memory.ram_usage)} / ${bytes(memory.ram_total)}`);
  memoryActivity(sample);
  const thermal = !vm && ["nominal", "fair", "serious", "critical"].includes(sample.thermal_state) ? sample.thermal_state : "unavailable";
  $("thermal-state").dataset.level = thermal;
  put("thermal-state", thermal === "unavailable" ? "Unavailable" : thermal[0].toUpperCase() + thermal.slice(1));
  put("device", [metrics.soc?.chip_name || "Local device", metrics.soc?.mac_model, vm ? "Virtual machine" : ""].filter(Boolean).join(" / "));
  put("sample-time", `Sample ${new Date(sample.sampled_at).toLocaleTimeString("en-US", { hour12: false })}`);
  state.history.push([ratio(metrics.cpu_active_ratio), ratio(gpu), ram, ...power(metrics), ...["cpu", "gpu"].map((name) => vm ? null : metrics.temp?.[`${name}_temp_avg`]), ...weighted]);
  if (state.history.length > 60) state.history.shift();
  network(metrics);
  disks(sample);
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
$("disk-select").addEventListener("change", () => { state.diskSelected = state.disks.has($("disk-select").value) ? $("disk-select").value : null; drawDiskChart(); });
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
new ResizeObserver(drawCharts).observe($("dashboard"));
applyDetails();
poll();
