// Executes the actual dashboard script; no browser, server, network, or config access.
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import vm from 'node:vm';

const sourcePath = fileURLToPath(new URL('../src/app/dashboard/app.js', import.meta.url));
const source = readFileSync(sourcePath, 'utf8');
const html = readFileSync(join(dirname(sourcePath), 'index.html'), 'utf8');

class Target {
  listeners = new Map();
  addEventListener(name, callback) {
    if (!this.listeners.has(name)) this.listeners.set(name, []);
    this.listeners.get(name).push(callback);
  }
  emit(name) { for (const callback of this.listeners.get(name) || []) callback({ type: name, target: this }); }
}

class Element extends Target {
  constructor(tag = 'div', className = '') {
    super();
    Object.assign(this, { tagName: tag.toUpperCase(), className, children: [], parentElement: null,
      style: {}, dataset: {}, attributes: new Map(), hidden: false, checked: false, _text: '', _width: 300, _height: 74, scrollTop: 0 });
  }
  get clientWidth() { return this.visible ? this._width : 0; }
  get clientHeight() { return this.visible ? this._height : 0; }
  get visible() { return !this.hidden && (!this.parentElement || this.parentElement.visible); }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  focus() { this.ownerDocument.activeElement = this; }
  get textContent() { return this._text + this.children.map((child) => child.textContent).join(''); }
  set textContent(value) { this.replaceChildren(); this._text = String(value); }
  set innerHTML(_) { throw new Error('Unsafe innerHTML reached the DOM stub'); }
  insertAdjacentHTML() { throw new Error('Unsafe insertAdjacentHTML reached the DOM stub'); }
  append(...nodes) {
    for (const node of nodes) {
      if (node.parentElement) node.remove();
      node.parentElement = this;
      this.children.push(node);
    }
  }
  replaceChildren(...nodes) {
    for (const child of this.children) child.parentElement = null;
    this.children = [];
    this._text = '';
    this.append(...nodes);
  }
  remove() {
    if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((node) => node !== this);
    this.parentElement = null;
  }
  querySelector(selector) {
    assert.ok(selector.startsWith('.'), `DOM stub needs selector support: ${selector}`);
    const name = selector.slice(1);
    for (const child of this.children) {
      if (child.className.split(/\s+/).includes(name)) return child;
      const descendant = child.querySelector(selector);
      if (descendant) return descendant;
    }
    return null;
  }
  cloneNode(deep) {
    const clone = new Element(this.tagName, this.className);
    clone._text = this._text;
    clone.style = { ...this.style };
    if (deep) clone.append(...this.children.map((node) => node.cloneNode(true)));
    return clone;
  }
  get firstElementChild() { return this.children[0] || null; }
  getContext(kind) {
    assert.equal(kind, '2d');
    this.context ??= { paths: [], dash: [],
      setTransform() { this.paths = []; },
      beginPath() { this.points = []; },
      moveTo(x, y) { this.points.push(['moveTo', x, y]); },
      lineTo(x, y) { this.points.push(['lineTo', x, y]); },
      setLineDash(dash) { this.dash = [...dash]; },
      stroke() { this.paths.push({ color: this.strokeStyle, dash: [...this.dash], points: this.points }); },
    };
    return this.context;
  }
}

function fanTemplate() {
  assert.match(html, /<template id="fan-template">/);
  const classes = Array.from(html.matchAll(/class="([^"]+)"/g), (match) => match[1].split(/\s+/));
  for (const name of ['rotor', 'fan-name', 'fan-rpm', 'fan-limit']) assert.ok(classes.some((tokens) => tokens.includes(name)));
  const card = new Element('article', 'fan-card');
  const housing = new Element('div', 'housing');
  housing.append(new Element('div', 'rotor'));
  const labels = new Element('div');
  labels.append(new Element('h3', 'fan-name'), new Element('p', 'fan-rpm'), new Element('p', 'muted fan-limit'));
  card.append(housing, labels);
  const content = new Element('template-content');
  content.append(card);
  return content;
}

async function drainMicrotasks() { for (let i = 0; i < 12; i++) await Promise.resolve(); }

function boot({ reduced = false, storage = new Map(), storageFailure = false, expanded = false } = {}) {
  let now = 0, nextId = 1, activeFetches = 0, maximumFetches = 0;
  const timers = new Map(), frames = new Map(), requests = [], elements = new Map();
  const document = new Target();
  Object.assign(document, { hidden: false, activeElement: null, documentElement: new Element('root'),
    getElementById(id) { assert.ok(elements.has(id), `Unknown DOM id: ${id}`); return elements.get(id); },
    createElement(tag) { const element = new Element(tag); element.ownerDocument = document; return element; } });
  const stack = [document.documentElement];
  for (const token of html.matchAll(/<\/?([a-z][\w-]*)\b([^>]*?)>|([^<]+)/gi)) {
    if (token[3]) { stack.at(-1)._text += token[3].replaceAll('&mdash;', '—').replaceAll('&amp;', '&'); continue; }
    const tag = token[1].toLowerCase();
    if (token[0].startsWith('</')) { if (stack.at(-1).tagName === tag.toUpperCase()) stack.pop(); continue; }
    const element = document.createElement(tag);
    const attributes = new Map([...token[2].matchAll(/([\w-]+)(?:="([^"]*)")?/g)].map((match) => [match[1], match[2] ?? '']));
    for (const [name, value] of attributes) {
      element.setAttribute(name, value);
      if (name.startsWith('data-')) element.dataset[name.slice(5)] = value;
    }
    element.className = attributes.get('class') || '';
    element.hidden = attributes.has('hidden'); element.disabled = attributes.has('disabled');
    if (attributes.has('id')) elements.set(attributes.get('id'), element);
    stack.at(-1).append(element);
    if (!['meta', 'link', 'input', 'br'].includes(tag)) stack.push(element);
  }
  elements.get('fan-template').content = fanTemplate();
  const media = new Target(); media.matches = reduced;
  const timer = (callback, delay = 0) => {
    const id = nextId++;
    timers.set(id, { at: now + Math.max(0, Number(delay)), callback });
    return id;
  };
  const fetch = (url, options) => {
    assert.ok(['/api/sample', '/api/interval'].includes(url));
    if (url === '/api/sample') assert.equal(options.cache, 'no-store');
    else {
      assert.equal(options.method, 'POST');
      assert.equal(options.headers['Content-Type'], 'application/json');
      assert.ok(['increase', 'decrease'].includes(JSON.parse(options.body).adjustment));
    }
    assert.ok(!requests.some((request) => request.url === url && !request.done), `Concurrent ${url}`);
    const request = { url, options, done: false, jsonCalls: 0 };
    activeFetches++;
    maximumFetches = Math.max(maximumFetches, activeFetches);
    request.promise = new Promise((resolve, reject) => {
      const finish = (value, error = false) => {
        if (request.done) return;
        request.done = true;
        activeFetches--;
        if (error) reject(value); else resolve(value);
      };
      request.answer = (payload, status = 200) => finish({ status, ok: status >= 200 && status < 300,
        async json() { request.jsonCalls++; return payload; } });
      request.fail = () => finish(new Error('Synthetic network failure'), true);
      options.signal.addEventListener('abort', () => {
        request.aborted = options.signal.reason;
        finish(Object.assign(new Error('Synthetic aborted fetch'), { name: 'AbortError' }), true);
      });
      if (options.signal.aborted) finish(new Error('Already aborted'), true);
    });
    requests.push(request);
    return request.promise;
  };
  const context = vm.createContext({ document, window: { devicePixelRatio: 2 }, console, AbortController,
    localStorage: { getItem(key) { if (storageFailure) throw new Error('Storage denied'); return storage.get(key) ?? null; },
      setItem(key, value) { if (storageFailure) throw new Error('Storage denied'); storage.set(key, value); } },
    performance: { now: () => now }, fetch, matchMedia: () => media,
    getComputedStyle: () => ({ getPropertyValue: (name) => name === '--power-subtotal' ? '#c1a4e8' : '#72d8e8' }),
    ResizeObserver: class { observe() {} },
    setTimeout: timer, clearTimeout: (id) => timers.delete(id),
    requestAnimationFrame(callback) { const id = nextId++; frames.set(id, callback); return id; },
    cancelAnimationFrame: (id) => frames.delete(id) });
  new vm.Script(source, { filename: sourcePath }).runInContext(context, { timeout: 1000 });
  if (expanded) elements.get('expand-all').emit('click');
  const inspect = (expression) => vm.runInContext(expression, context, { timeout: 1000 });
  const pending = (url = '/api/sample') => requests.find((request) => request.url === url && !request.done);
  const advance = async (milliseconds) => {
    const until = now + milliseconds;
    let iterations = 0;
    while (true) {
      const next = [...timers].filter(([, value]) => value.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) break;
      assert.ok(iterations++ < 1000, 'Runaway synthetic timer loop');
      now = Math.max(now, next[1].at);
      timers.delete(next[0]);
      next[1].callback();
      await drainMicrotasks();
    }
    now = until;
    await drainMicrotasks();
  };
  return {
    elements, document, media, frames, requests, inspect, pending, storage,
    state: () => inspect('state'),
    text: (id) => elements.get(id).textContent,
    status: () => elements.get('status').dataset.state,
    now: () => now,
    maximumFetches: () => maximumFetches,
    async answer(payload, status = 200) { assert.ok(pending(), 'No pending real poll'); pending().answer(payload, status); await drainMicrotasks(); },
    async control(payload, status = 200) { assert.ok(pending('/api/interval'), 'No pending real control'); pending('/api/interval').answer(payload, status); await drainMicrotasks(); },
    async fail() { assert.ok(pending()); pending().fail(); await drainMicrotasks(); },
    async click(id) { const element = elements.get(id); if (!element.disabled) element.emit('click'); await drainMicrotasks(); },
    advance,
    async frame(milliseconds = 1000 / 60) {
      await advance(milliseconds);
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(now);
      await drainMicrotasks();
    },
    async motion(enabled) { elements.get('motion').checked = enabled; elements.get('motion').emit('change'); await drainMicrotasks(); },
    async reduced(enabled) { media.matches = enabled; media.emit('change'); await drainMicrotasks(); },
    async hidden(enabled) { document.hidden = enabled; document.emit('visibilitychange'); await drainMicrotasks(); },
  };
}

function metrics(overrides = {}) {
  return { soc: { chip_name: 'Apple M2 Max', mac_model: 'MacBook Pro', vm: false },
    cpu_active_ratio: 0.25, gpu_active_ratio: 0.5, cpu_power: 8, gpu_power: 4,
    ane_power: 0, ram_power: 0.7, gpu_ram_power: 0.0005, all_power: 12, sys_power: 20,
    gpu_freq_mhz: 900, cpu_tiers: [{ label: 'E', freq_mhz: 1200 }, { label: 'P', freq_mhz: 2400 }],
    memory: { ram_usage: 8 * 1024 ** 3, ram_total: 32 * 1024 ** 3, swap_usage: 0 },
    temp: { cpu_temp_avg: 45, gpu_temp_avg: 43 },
    network: [{ name: 'en0', rx_bytes_per_sec: 1000, tx_bytes_per_sec: 500, rx_bytes: 4096, tx_bytes: 2048 }],
    fans: [{ name: 'fan0', rpm: 1800, max_rpm: 6800 }, { name: 'fan1', rpm: 2400 }], ...overrides };
}
function sample({ sequence = 1, sampled_at = '2026-10-14T12:00:00Z', age_ms = 0, interval_ms = 1000, cadence, data = metrics(), processes = null, processes_age_ms = null, temperatures } = {}) {
  return { sequence, sampled_at, age_ms, interval_ms, cadence, metrics: data, processes, processes_age_ms, temperatures };
}
const cadence = (requested_ms = 1000, effective_ms = requested_ms) => ({ requested_ms, effective_ms, pending: requested_ms !== effective_ms });
async function nextPoll(harness) {
  for (let i = 0; !harness.pending() && i < 4; i++) await harness.advance(1000);
  assert.ok(harness.pending(), 'Expected the actual poll scheduler to fetch');
}
async function animate(harness, count = 30) { for (let i = 0; i < count; i++) await harness.frame(); }
const tests = [];
async function check(name, test) { await test(); tests.push(name); }

await check('missing metrics and VM never invent telemetry', async () => {
  const h = boot();
  await h.answer(sample({ data: { soc: { chip_name: 'VM', vm: true }, network: null,
    fans: [{ name: 'not-a-real-vm-fan', rpm: 9999 }], cpu_power: 99, gpu_power: 88,
    gpu_active_ratio: 0, gpu_freq_mhz: 0 } }));
  assert.equal(h.text('cpu-value'), '—');
  assert.equal(h.text('gpu-value'), '—');
  assert.equal(h.state().history[0][1], null);
  assert.equal(h.text('ram-value'), '—');
  assert.ok(!h.text('cpu-detail').includes('99'));
  assert.ok(!h.text('gpu-detail').includes('88') && !h.text('gpu-detail').includes('0 MHz'));
  assert.equal(h.elements.get('network-table').hidden, true);
  assert.match(h.text('network-empty'), /unavailable/i);
  assert.equal(h.state().fans.size, 0);
  assert.equal(h.frames.size, 0);
  await nextPoll(h);
  await h.answer(sample({ sequence: 2, data: {} }));
  assert.match(h.text('network-empty'), /unavailable/i, 'An omitted network field must not claim zero interfaces');
  assert.equal(h.state().fans.size, 0);
});

await check('unavailable GPU tables stay distinct from measured idle GPU activity', async () => {
  const h = boot();
  await h.answer(sample({ data: metrics({ soc: { vm: false, gpu_freqs: [] }, gpu_active_ratio: 0 }) }));
  assert.equal(h.text('gpu-value'), '—');
  assert.equal(h.state().history[0][1], null);
  assert.ok(!h.text('gpu-detail').includes('900 MHz'));
  await nextPoll(h);
  await h.answer(sample({ sequence: 2, data: metrics({ soc: { vm: false, gpu_freqs: [0] }, gpu_active_ratio: 0 }) }));
  assert.equal(h.text('gpu-value'), '—');
  await nextPoll(h);
  await h.answer(sample({ sequence: 3, data: metrics({ soc: { vm: false, gpu_freqs: [400] }, gpu_active_ratio: 0 }) }));
  assert.equal(h.text('gpu-value'), '—');
  await nextPoll(h);
  await h.answer(sample({ sequence: 4, data: metrics({ soc: { vm: false, gpu_freqs: [0, 400, 1400] }, gpu_active_ratio: 0 }) }));
  assert.equal(h.text('gpu-value'), '0.0%');
  assert.equal(h.state().history.at(-1)[1], 0);
});

await check('a delayed response cannot label an already expired sample live', async () => {
  const h = boot();
  await h.advance(2000);
  await h.answer(sample({ age_ms: 2000 }));
  assert.equal(h.status(), 'stale');
  assert.equal(h.frames.size, 0);
  await nextPoll(h);
  await h.answer(sample({ sequence: 2 }));
  assert.equal(h.status(), 'live');
});

await check('negative, unknown, zero and sub-byte network rates stay distinct', async () => {
  const h = boot();
  await h.answer(sample({ data: metrics({ network: [
    { name: 'fractional', rx_bytes_per_sec: 0.5, tx_bytes_per_sec: 0, rx_bytes: 1, tx_bytes: 0 },
    { name: 'unknown', rx_bytes_per_sec: -1, tx_bytes_per_sec: null, rx_bytes: -1, tx_bytes: null },
  ] }) }));
  const rows = h.elements.get('network-rows').children;
  assert.equal(rows[0].children[1].textContent, '0.5 B');
  assert.equal(rows[0].children[2].textContent, '0 B');
  assert.equal(rows[1].children[1].textContent, '—');
  assert.equal(rows[1].children[2].textContent, '—');
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ network: [] }) }));
  assert.match(h.text('network-empty'), /No active interfaces/i);
  assert.equal(h.elements.get('network-table').hidden, true);
});

await check('malicious device/interface/fan names only reach textContent', async () => {
  const h = boot(), name = '<img src=x onerror="globalThis.pwned=1">';
  await h.answer(sample({ data: metrics({ soc: { chip_name: name, mac_model: name },
    network: [{ name, rx_bytes_per_sec: 1, tx_bytes_per_sec: 2 }], fans: [{ name, rpm: 1000 }] }) }));
  assert.equal(h.status(), 'live');
  assert.equal(h.elements.get('network-rows').children[0].children[0].textContent, name);
  assert.equal(h.state().fans.get(name).node.querySelector('.fan-name').textContent, name);
  assert.ok(h.text('device').includes(name));
  assert.equal(h.inspect('globalThis.pwned'), undefined);
});

await check('real fan identities retain node and phase through reordering', async () => {
  const h = boot({ expanded: true }); await h.answer(sample()); await animate(h);
  const slow = h.state().fans.get('fan0'), fast = h.state().fans.get('fan1');
  assert.ok(fast.angle > slow.angle && slow.angle > 0);
  assert.equal(slow.rotor.children.length, 7);
  const angle = slow.angle, fastAngle = fast.angle, slowNode = slow.node;
  await nextPoll(h);
  await h.answer(sample({ sequence: 2, data: metrics({ fans: [
    { name: 'fan1', rpm: 2400 }, { name: 'fan0', rpm: 1800 }, { name: 'new-fan', rpm: 0 },
  ] }) }));
  assert.equal(h.state().fans.get('fan0').node, slowNode);
  assert.equal(h.state().fans.get('fan0').angle, angle);
  assert.equal(h.state().fans.get('fan1').angle, fastAngle);
  assert.equal(h.state().fans.get('new-fan').angle, 0);
  assert.deepEqual(Array.from(h.state().fans.keys()), ['fan1', 'fan0', 'new-fan']);
  await animate(h, 10);
  assert.equal(h.state().fans.get('new-fan').angle, 0);
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: metrics({ fans: [{ name: 'fan0', rpm: 0 }] }) }));
  const stopped = h.state().fans.get('fan0').angle;
  assert.equal(h.state().fans.size, 1); assert.equal(h.frames.size, 0);
  await animate(h, 10); assert.equal(h.state().fans.get('fan0').angle, stopped);
  assert.equal(h.state().fans.get('fan0').node.querySelector('.fan-rpm').textContent, '0 RPM');
});

await check('same identity does not append history; a restart with the same sequence does', async () => {
  const h = boot({ expanded: true }); await h.answer(sample({ sequence: 4 }));
  await nextPoll(h); await h.answer(sample({ sequence: 4, age_ms: 1000, data: metrics({ sys_power: 1000000 }) }));
  assert.equal(h.state().history.length, 1);
  assert.equal(h.text('system-power'), '20 W');
  assert.equal(h.text('power-range'), '0–20 W');
  await nextPoll(h); await h.answer(sample({ sequence: 4, sampled_at: '2026-10-14T12:05:00Z' }));
  assert.equal(h.state().history.length, 2);
  for (let sequence = 5; sequence < 70; sequence++) {
    await nextPoll(h); await h.answer(sample({ sequence, sampled_at: '2026-10-14T12:05:00Z', data: metrics({ sys_power: sequence, all_power: sequence / 2 }) }));
  }
  assert.equal(h.state().history.length, 60);
  assert.equal(h.state().history[0][3], 10);
  assert.equal(h.state().history.at(-1)[3], 69);
  assert.equal(h.elements.get('power-chart').context.paths.at(-1).points.length, 60);
  assert.equal(h.text('power-range'), '0–100 W');
  assert.equal(h.maximumFetches(), 1);
});

await check('503 waiting, offline and stale states recover without erasing the last values', async () => {
  const h = boot({ expanded: true }); await h.answer(null, 503);
  assert.equal(h.status(), 'waiting'); assert.equal(h.requests[0].jsonCalls, 0);
  assert.equal(h.frames.size, 0); assert.equal(h.state().history.length, 0);
  await nextPoll(h); await h.answer(sample());
  assert.equal(h.status(), 'live'); const cpu = h.text('cpu-value');
  await nextPoll(h); await h.fail();
  assert.equal(h.status(), 'offline'); assert.equal(h.text('cpu-value'), cpu); assert.equal(h.frames.size, 0);
  await nextPoll(h); await h.answer(sample({ sequence: 2, age_ms: 2990 }));
  assert.equal(h.status(), 'live'); await h.advance(12);
  assert.equal(h.status(), 'stale'); assert.equal(h.frames.size, 0); assert.equal(h.text('cpu-value'), cpu);
  await nextPoll(h); await h.answer(sample({ sequence: 3 }));
  assert.equal(h.status(), 'live'); assert.equal(h.frames.size, 1);
});

await check('manual and reduced motion stop only fan animation', async () => {
  const h = boot({ expanded: true }); await h.answer(sample()); await animate(h, 10);
  const fan = h.state().fans.get('fan0'), angle = fan.angle;
  await h.motion(false); assert.equal(h.frames.size, 0);
  await animate(h, 10); assert.equal(fan.angle, angle);
  await nextPoll(h); await h.answer(sample({ sequence: 2 }));
  assert.equal(h.status(), 'live'); assert.equal(h.frames.size, 0);
  await h.motion(true); assert.equal(h.frames.size, 1); await animate(h, 10);
  assert.ok(fan.angle > angle);
  await h.reduced(true); assert.equal(h.frames.size, 0); assert.equal(h.elements.get('motion').checked, false);
  await nextPoll(h); await h.answer(sample({ sequence: 3 }));
  assert.equal(h.status(), 'live'); assert.equal(h.frames.size, 0);
  const initial = boot({ reduced: true, expanded: true }); await initial.answer(sample());
  assert.equal(initial.frames.size, 0); assert.equal(initial.elements.get('motion').checked, false);
});

await check('hidden tabs cancel animation and polling, then refresh on return', async () => {
  const h = boot({ expanded: true }); await h.answer(sample()); await animate(h, 5);
  const angle = h.state().fans.get('fan0').angle, count = h.requests.length;
  await h.hidden(true); assert.equal(h.frames.size, 0);
  await h.advance(10000); assert.equal(h.requests.length, count);
  assert.equal(h.state().fans.get('fan0').angle, angle);
  await h.hidden(false); await h.advance(0);
  assert.ok(h.pending()); await h.answer(sample({ sequence: 2 }));
  assert.equal(h.status(), 'live'); assert.equal(h.frames.size, 1);
  await animate(h, 2); assert.ok(h.state().fans.get('fan0').angle > angle);
});

await check('hidden in-flight abort and rapid visibility changes preserve single flight', async () => {
  const h = boot(); assert.ok(h.pending());
  await h.hidden(true); assert.equal(h.requests[0].aborted, 'hidden');
  await h.hidden(false); await h.advance(0);
  assert.equal(h.requests.length, 2);
  h.document.emit('visibilitychange'); h.document.emit('visibilitychange');
  h.inspect('poll(); poll();'); await drainMicrotasks();
  assert.equal(h.requests.length, 2); assert.equal(h.maximumFetches(), 1);
  await h.answer(sample()); await h.advance(0);
  assert.equal(h.maximumFetches(), 1);
  assert.ok(h.requests.length <= 3);
});

await check('fetch timeout and invalid envelopes stay bounded and recover', async () => {
  const h = boot(); await h.advance(2501);
  assert.equal(h.requests[0].done, true); assert.equal(h.status(), 'offline');
  await nextPoll(h); await h.answer({ ...sample(), sequence: 'not-an-integer' });
  assert.equal(h.status(), 'offline'); assert.equal(h.state().history.length, 0);
  await nextPoll(h); await h.answer(sample());
  assert.equal(h.status(), 'live'); assert.equal(h.maximumFetches(), 1);
});

await check('all interfaces remain available and busiest interfaces sort first without mutating the sample', async () => {
  const h = boot();
  const network = Array.from({ length: 21 }, (_, index) => ({
    name: `en${index}`, rx_bytes_per_sec: index, tx_bytes_per_sec: index * 2,
  }));
  const before = structuredClone(network);
  await h.answer(sample({ data: metrics({ network }) }));
  const rows = h.elements.get('network-rows').children;
  assert.equal(rows.length, 21);
  assert.equal(rows[0].children[0].textContent, 'en20');
  assert.equal(rows.at(-1).children[0].textContent, 'en0');
  assert.deepEqual(network, before);
});

await check('named temperature readings retain activity and distinguish missing sensors', async () => {
  const h = boot(); await h.answer(sample());
  assert.equal(h.text('cpu-value'), '25.0%'); assert.equal(h.text('gpu-value'), '50.0%');
  assert.equal(h.text('cpu-temp'), '45.0 °C'); assert.equal(h.text('gpu-temp'), '43.0 °C');
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ temp: { cpu_temp_avg: 0, gpu_temp_avg: null } }) }));
  assert.equal(h.text('cpu-temp'), '0.0 °C'); assert.equal(h.text('gpu-temp'), 'Unavailable');
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: {} }));
  assert.equal(h.text('cpu-temp'), 'Unavailable'); assert.equal(h.text('gpu-temp'), 'Unavailable');
  assert.match(html, /CPU TEMP/); assert.match(html, /GPU TEMP/);
});

await check('power keeps provided totals separate from memory and retains tiny positive readings', async () => {
  const h = boot();
  await h.answer(sample({ data: metrics({ cpu_power: 8, gpu_power: 4, ane_power: 1.25,
    ram_power: 7, gpu_ram_power: 0.00003, all_power: 13.25, sys_power: 40,
    soc: { vm: false, gpu_freqs: [] } }) }));
  assert.equal(h.text('system-power'), '40 W');
  assert.equal(h.text('compute-power'), '13.25 W', 'DRAM and GPU SRAM do not increase the supplied subtotal');
  assert.equal(h.text('cpu-power'), '8 W'); assert.equal(h.text('gpu-power'), '4 W');
  assert.equal(h.text('ane-power'), '1.25 W'); assert.equal(h.text('ram-power'), '7 W');
  assert.equal(h.text('gpu_ram-power'), '0.03 mW', 'Positive SRAM power must not round to zero');
  assert.equal(h.text('gpu-value'), '—', 'Unavailable frequency tables do not hide reported GPU power');
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ cpu_power: 0.00001, gpu_power: 0.125 }) }));
  assert.equal(h.text('cpu-power'), '0.01 mW'); assert.equal(h.text('gpu-power'), '125 mW');
  assert.match(h.text('cpu-detail'), /^0.01 mW/); assert.match(h.text('gpu-detail'), /^125 mW/);
  assert.match(html, /unsupported counters report zero/); assert.match(html, /not wall-socket power/);
});

await check('power distinguishes reported zeros, invalid readings and VM placeholders', async () => {
  const h = boot();
  const ids = ['cpu-power', 'gpu-power', 'ane-power', 'ram-power', 'gpu_ram-power', 'compute-power'];
  const zeros = { cpu_power: 0, gpu_power: 0, ane_power: 0, ram_power: 0, gpu_ram_power: 0, all_power: 0, sys_power: 0 };
  await h.answer(sample({ data: metrics(zeros) }));
  for (const id of ids) assert.equal(h.text(id), '0 W');
  assert.equal(h.text('system-power'), 'Unavailable');
  assert.equal(h.text('power-range'), '0–1 W');
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ cpu_power: null, gpu_power: -1,
    ane_power: NaN, ram_power: Infinity, gpu_ram_power: '5', all_power: undefined, sys_power: -Infinity }) }));
  for (const id of [...ids, 'system-power']) assert.equal(h.text(id), 'Unavailable');
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: metrics({ soc: { vm: true } }) }));
  for (const id of [...ids, 'system-power']) assert.equal(h.text(id), 'Unavailable');
  assert.ok(Array.from(h.state().history.at(-1).slice(3)).every((value) => value === null));
  const absent = boot(); await absent.answer(sample({ data: {} }));
  for (const id of [...ids, 'system-power']) assert.equal(absent.text(id), 'Unavailable');
  assert.equal(absent.text('power-range'), 'Scale unavailable');
});

await check('power chart uses separate shared watt-scaled lines and breaks across missing samples', async () => {
  const h = boot({ expanded: true }), traces = () => h.elements.get('power-chart').context.paths.slice(-2);
  await h.answer(sample({ data: metrics({ sys_power: 40, all_power: 10 }) }));
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ sys_power: 50, all_power: 20 }) }));
  assert.equal(h.text('power-range'), '0–50 W');
  const [system, compute] = traces();
  assert.notEqual(system.color, compute.color); assert.deepEqual(system.dash, []); assert.deepEqual(compute.dash, [5, 4]);
  assert.deepEqual(system.points.map((point) => point[0]), ['moveTo', 'lineTo']);
  assert.ok(Math.abs(system.points[0][2] - 14.8) < 0.001);
  assert.equal(system.points[1][2], 0);
  assert.ok(Math.abs(compute.points[1][2] - 44.4) < 0.001, '20 W is 40% of the same 50 W range, not a stacked total');
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: metrics({ sys_power: 0, all_power: null }) }));
  await nextPoll(h); await h.answer(sample({ sequence: 4, data: metrics({ sys_power: 25, all_power: 5 }) }));
  for (const trace of traces()) assert.deepEqual(trace.points.map((point) => point[0]), ['moveTo', 'lineTo', 'moveTo']);
});

await check('all processes sort by real usage with missing rates last and safe names', async () => {
  const h = boot(), name = '<img onerror="globalThis.pwned=1">';
  const processes = Array.from({ length: 80 }, (_, i) => ({ pid: i + 1, name: i ? `process${i}` : name,
    cpu_pct: i ? i * 3 : null, gpu_pct: i === 79 ? null : i, mem_bytes: 1000 - i }));
  const before = structuredClone(processes);
  await h.answer(sample({ processes, processes_age_ms: 25 }));
  const rows = () => h.elements.get('process-rows').children;
  assert.equal(rows().length, 80); assert.equal(h.elements.get('process-table').hidden, false);
  assert.equal(rows()[0].children[1].textContent, '237.0%', 'per-core CPU may exceed 100%');
  assert.equal(rows().at(-1).children[0].textContent, `${name}PID 1`);
  assert.equal(rows().at(-1).children[1].textContent, '—');
  const requests = h.requests.length;
  h.elements.get('process-sort').value = 'gpu_pct'; h.elements.get('process-sort').emit('change');
  assert.equal(rows()[0].children[0].textContent, 'process78PID 79');
  assert.equal(rows().at(-1).children[2].textContent, '—');
  h.elements.get('process-sort').value = 'mem_bytes'; h.elements.get('process-sort').emit('change');
  assert.equal(rows()[0].children[0].textContent, `${name}PID 1`);
  assert.equal(h.requests.length, requests, 'sorting does not request or resample');
  assert.deepEqual(processes, before); assert.equal(h.inspect('globalThis.pwned'), undefined);
  assert.match(h.text('process-status'), /0s old.*one core/);
});

await check('process snapshots distinguish local-only, empty, stale and retained offline values', async () => {
  const h = boot(); await h.answer(sample());
  assert.equal(h.elements.get('process-table').hidden, true);
  assert.match(h.text('process-empty'), /loopback/);
  await nextPoll(h); await h.answer(sample({ sequence: 2, processes: [], processes_age_ms: 0 }));
  assert.match(h.text('process-empty'), /No processes/);
  await nextPoll(h); await h.answer(sample({ sequence: 3, processes: [{ pid: 1, name: 'real', cpu_pct: 0, gpu_pct: null, mem_bytes: 123 }], processes_age_ms: 5000 }));
  assert.match(h.text('process-status'), /STALE/);
  const retained = h.text('process-rows');
  await nextPoll(h); await h.fail();
  assert.equal(h.status(), 'offline'); assert.equal(h.text('process-rows'), retained);
  assert.match(h.text('process-status'), /^OFFLINE \/ retained/);
  await nextPoll(h); await h.answer(sample(), 503);
  assert.match(h.text('process-status'), /^WAITING \/ retained/);
});

await check('interval controls use intentional POSTs and show actual effective settings', async () => {
  const h = boot();
  assert.equal(h.elements.get('interval-up').disabled, true);
  await h.answer(sample({ interval_ms: 100, cadence: cadence(100) }));
  assert.equal(h.text('interval-value'), '100 ms', 'Old CLI lower bound remains visible');
  await h.click('interval-up');
  assert.equal(h.elements.get('interval-up').disabled, true);
  assert.equal(h.text('interval-value'), '100 ms', 'No optimistic backend value');
  assert.match(h.text('interval-feedback'), /Applying/);
  assert.equal(JSON.parse(h.pending('/api/interval').options.body).adjustment, 'increase');
  await h.control(cadence(250, 100));
  assert.match(h.text('interval-feedback'), /Pending 250 ms/);
  assert.equal(h.elements.get('interval-up').disabled, false);
  assert.equal(h.elements.get('interval-down').disabled, true);
  assert.ok(h.pending(), 'POST completion immediately refreshes the sample');
  await h.answer(sample({ interval_ms: 100, cadence: cadence(250) }));
  assert.equal(h.text('interval-value'), '250 ms');
  assert.match(h.text('cadence-footer'), /250 ms sampling/);
  assert.equal(h.state().history.length, 1, 'Same sample refreshes cadence without extra history');
  await h.advance(249); assert.equal(h.pending(), undefined);
  await h.advance(1); assert.ok(h.pending(), 'Effective 250 ms also changes poll cadence');
});

await check('pending intervals stay adjustable while a long window is finishing', async () => {
  const h = boot(); await h.answer(sample({ interval_ms: 10000, cadence: cadence(10000) }));
  assert.equal(h.elements.get('interval-up').disabled, true);
  await h.click('interval-down'); await h.control(cadence(9750, 10000));
  assert.match(h.text('interval-feedback'), /Pending 9750 ms/);
  assert.equal(h.text('interval-value'), '10000 ms');
  assert.equal(h.elements.get('interval-down').disabled, false);
  assert.equal(h.elements.get('interval-up').disabled, false);
  await h.click('interval-down');
  assert.ok(h.pending('/api/interval'), 'No need to wait for a completed sample to adjust again');
  await h.control(cadence(9500, 10000));
  await h.answer(sample({ interval_ms: 10000, cadence: cadence(9500, 10000) }));
  await h.advance(0);
  if (h.pending()) await h.answer(sample({ interval_ms: 10000, cadence: cadence(9500, 10000) }));
  await h.advance(999); assert.equal(h.pending(), undefined);
  await h.advance(1); assert.ok(h.pending(), 'Pending long acquisitions are checked at least once a second');
  assert.equal(h.text('interval-value'), '10000 ms');
  assert.equal(h.state().history.length, 1);
});

await check('POST rejection and timeout keep the last server value and release controls', async () => {
  const h = boot(); await h.answer(sample({ cadence: cadence() }));
  await h.click('interval-up'); await h.control({ error: 'forbidden' }, 403);
  assert.equal(h.text('interval-value'), '1000 ms');
  assert.match(h.text('interval-feedback'), /not confirmed/);
  assert.equal(h.elements.get('interval-up').disabled, false);
  await h.answer(sample({ cadence: cadence() }));
  await h.click('interval-down');
  await h.advance(2501);
  assert.equal(h.pending('/api/interval'), undefined);
  assert.equal(h.text('interval-value'), '1000 ms');
  assert.match(h.text('interval-feedback'), /not confirmed/);
  assert.equal(h.elements.get('interval-down').disabled, false);
  if (h.pending()) await h.answer(sample({ cadence: cadence(750, 1000) }));
  await h.click('interval-up'); await h.control(cadence());
  assert.match(h.text('interval-feedback'), /Service sampling/);
  assert.equal(h.state().changing, false);
});

await check('only actual in-flight cadence can extend freshness, never a pending request', async () => {
  const stuck = boot(); await stuck.answer(sample({ interval_ms: 250, cadence: cadence(10000, 250) }));
  for (let elapsed = 250; elapsed <= 3000; elapsed += 250) {
    await stuck.advance(250);
    await stuck.answer(sample({ interval_ms: 250, age_ms: elapsed, cadence: cadence(10000, 250) }));
  }
  await stuck.advance(1);
  assert.equal(stuck.status(), 'stale', 'A sampler that never begins cannot remain LIVE by accepting requests');
  assert.equal(stuck.elements.get('interval-down').disabled, false);
  assert.match(stuck.text('interval-feedback'), /Pending 10000 ms/);
  const active = boot({ expanded: true }); await active.answer(sample({ interval_ms: 250, cadence: cadence(10000, 250) }));
  await active.advance(250);
  await active.answer(sample({ interval_ms: 250, age_ms: 250, cadence: cadence(10000) }));
  assert.equal(active.state().history.length, 1);
  await active.advance(4000);
  assert.equal(active.status(), 'live', 'A confirmed long acquisition legitimately has a longer deadline');
  assert.equal(active.pending(), undefined);
  assert.match(active.text('interval-value'), /10000 ms/);
  const angle = active.state().fans.get('fan0').angle;
  await animate(active, 30);
  assert.ok(active.state().fans.get('fan0').angle > angle, 'Fan rAF stays independent of 10 s polling');
  await active.advance(25001);
  assert.notEqual(active.status(), 'live', 'An indefinitely stuck long acquisition still expires');
});

await check('panels start compact and native buttons expose individual and all detail controls', async () => {
  const h = boot(), names = ['cpu', 'gpu', 'ram', 'power', 'network', 'cooling', 'process'];
  for (const name of names) {
    const button = h.elements.get(`${name}-toggle`), body = h.elements.get(`${name}-details`);
    assert.equal(button.tagName, 'BUTTON'); assert.equal(button.getAttribute('type'), 'button');
    assert.equal(button.getAttribute('aria-controls'), `${name}-details`);
    assert.equal(button.getAttribute('aria-expanded'), 'false');
    if (!['network', 'process'].includes(name)) assert.equal(body.hidden, true);
  }
  await h.click('cpu-toggle');
  assert.equal(h.elements.get('cpu-toggle').getAttribute('aria-expanded'), 'true');
  assert.equal(h.elements.get('cpu-details').hidden, false);
  assert.equal(h.elements.get('gpu-details').hidden, true);
  await h.click('expand-all');
  for (const name of names) assert.equal(h.elements.get(`${name}-toggle`).getAttribute('aria-expanded'), 'true');
  await h.click('collapse-all');
  for (const name of names) assert.equal(h.elements.get(`${name}-toggle`).getAttribute('aria-expanded'), 'false');
  assert.equal(h.pending().url, '/api/sample', 'View controls never start additional sampling requests');
});

await check('detail preferences reload independently and unavailable or corrupt storage stays usable', async () => {
  const storage = new Map(), first = boot({ storage });
  await first.click('network-toggle'); await first.click('cooling-toggle');
  const second = boot({ storage });
  assert.equal(second.state().details.network, true); assert.equal(second.state().details.cooling, true);
  assert.equal(second.state().details.process, false);
  for (const raw of ['invalid JSON', 'null', '42', '[]', '{"cpu":"true","gpu":true,"unexpected":true}']) {
    const h = boot({ storage: new Map([['macmon.panel-details.v1', raw]]) });
    assert.equal(h.state().details.cpu, false); assert.equal(h.state().details.gpu, raw.includes('"gpu":true'));
    await h.click('cpu-toggle'); assert.equal(h.state().details.cpu, true);
  }
  const denied = boot({ storageFailure: true }); await denied.click('expand-all');
  assert.equal(denied.state().details.cooling, true); await denied.answer(sample());
  assert.equal(denied.status(), 'live'); await denied.click('collapse-all');
  assert.equal(denied.state().details.cooling, false);
});

await check('compact tables retain busiest named interfaces and selected-order top processes without losing any rows', async () => {
  const h = boot(), network = Array.from({ length: 27 }, (_, i) => ({ name: `utun${i}`, rx_bytes_per_sec: i, tx_bytes_per_sec: i * 2 }));
  const processes = Array.from({ length: 80 }, (_, i) => ({ pid: i + 1, name: `process${i}`, cpu_pct: i, mem_bytes: 100 - i }));
  await h.answer(sample({ data: metrics({ network }), processes, processes_age_ms: 0 }));
  const visible = (name) => h.elements.get(`${name}-rows`).children.filter((row) => row.visible);
  assert.equal(visible('network').length, 3); assert.equal(visible('network')[0].children[0].textContent, 'utun26');
  assert.equal(visible('process').length, 5); assert.match(visible('process')[0].children[0].textContent, /^process79/);
  assert.equal(h.text('network-count'), '27 INTERFACES'); assert.equal(h.text('process-count'), '80 PROCESSES');
  assert.match(h.text('network-scope'), /rates are kept separate/); assert.match(h.text('process-scope'), /Top 5 of 80/);
  await h.click('network-toggle'); await h.click('process-toggle');
  assert.equal(visible('network').length, 27); assert.equal(visible('process').length, 80);
  h.elements.get('process-sort').value = 'mem_bytes'; h.elements.get('process-sort').emit('change');
  await h.click('collapse-all');
  assert.match(visible('process')[0].children[0].textContent, /^process0/);
  assert.equal(visible('network').length, 3); assert.equal(visible('process').length, 5);
  assert.equal(h.elements.get('network-rows').children.length, 27);
  assert.equal(h.elements.get('process-rows').children.length, 80);
});

await check('new samples and offline recovery retain controls, focus, sort, scrolling and detail choices', async () => {
  const h = boot(); await h.click('expand-all');
  const button = h.elements.get('network-toggle'), sort = h.elements.get('process-sort'), scroll = h.elements.get('network-details');
  sort.value = 'mem_bytes'; sort.focus(); scroll.scrollTop = 240;
  await h.answer(sample({ processes: [{ pid: 1, name: 'first', mem_bytes: 1 }, { pid: 2, name: 'second', mem_bytes: 2 }] }));
  await nextPoll(h); await h.answer(sample({ sequence: 2 }));
  assert.equal(h.elements.get('network-toggle'), button); assert.equal(h.elements.get('process-sort'), sort);
  assert.equal(h.document.activeElement, sort); assert.equal(sort.value, 'mem_bytes'); assert.equal(scroll.scrollTop, 240);
  await nextPoll(h); await h.fail();
  assert.equal(h.status(), 'offline'); assert.equal(button.getAttribute('aria-expanded'), 'true');
  assert.equal(h.document.activeElement, sort); assert.equal(scroll.scrollTop, 240);
  await h.click('cooling-toggle'); assert.equal(h.state().details.cooling, false);
  await nextPoll(h); await h.answer(sample({ sequence: 3 }));
  assert.equal(h.status(), 'live'); assert.equal(h.state().details.network, true); assert.equal(h.state().details.cooling, false);
  assert.equal(h.frames.size, 0);
});

await check('collapsed cooling stops animation while RPM and thermal summaries keep sampling, then resumes existing fans', async () => {
  const h = boot(); await h.answer(sample());
  assert.equal(h.frames.size, 0); assert.match(h.text('fan-summary'), /fan0 1800 RPM/);
  assert.equal(h.text('cooling-cpu-temp'), '45.0 °C'); assert.equal(h.elements.get('fans').visible, false);
  await h.click('cooling-toggle'); await animate(h, 10);
  const fan = h.state().fans.get('fan0'), angle = fan.angle;
  assert.ok(angle > 0); await h.click('cooling-toggle');
  assert.equal(h.frames.size, 0); await animate(h, 10); assert.equal(fan.angle, angle);
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ fans: [{ name: 'fan0', rpm: 3200 }], temp: { cpu_temp_avg: 55, gpu_temp_avg: 48 } }) }));
  assert.equal(h.state().fans.get('fan0'), fan); assert.equal(fan.angle, angle);
  assert.equal(h.text('fan-summary'), 'fan0 3200 RPM'); assert.equal(h.text('cooling-cpu-temp'), '55.0 °C');
  assert.equal(h.frames.size, 0); await h.click('cooling-toggle'); await animate(h, 10);
  assert.ok(fan.angle > angle); assert.equal(h.elements.get('temperature-chart').width, 600);
});

await check('raw temperatures show safe names and sources with current grouped ranges separate from historical mean peaks', async () => {
  const h = boot(), name = '<img src=x onerror="globalThis.pwned=1">';
  await h.answer(sample({ temperatures: [
    { name: 'TC0', group: 'cpu', source: 'smc', celsius: 51 },
    { name, group: 'cpu', source: 'hid', celsius: 61.5 },
    { name: 'TG0', group: 'gpu', source: 'smc', celsius: 150 },
    { name: 'bad', group: 'cpu', source: 'unknown', celsius: 90 },
    { name: 'bad', group: 'battery', source: 'hid', celsius: 90 },
    { name: 'bad', group: 'cpu', source: 'smc', celsius: NaN },
    { name: 'zero', group: 'cpu', source: 'smc', celsius: 0 },
    { name: 'negative', group: 'cpu', source: 'hid', celsius: -5 },
    { name: 'too hot', group: 'cpu', source: 'smc', celsius: 150.1 },
    { name: 'infinite', group: 'gpu', source: 'hid', celsius: Infinity },
  ] }));
  assert.equal(h.elements.get('temperature-table').visible, false); await h.click('cooling-toggle');
  const rows = h.elements.get('temperature-rows').children;
  assert.equal(rows.length, 3); assert.equal(rows[0].children[0].textContent, name);
  assert.equal(rows[0].children[2].textContent, 'HID'); assert.equal(rows[0].children[3].textContent, '61.5 °C');
  assert.match(h.text('temperature-groups'), /CPU: 2 sensors \/ current min 51.0 °C \/ max 61.5 °C/);
  assert.match(h.text('temperature-groups'), /GPU: 1 sensors \/ current min 150.0 °C \/ max 150.0 °C \/ hottest TG0/);
  assert.ok(h.text('temperature-groups').includes(`hottest ${name}`), 'The current hotspot keeps its raw safe-text name');
  assert.match(h.text('temperature-peaks'), /CPU 45.0 °C \/ GPU 43.0 °C/);
  assert.match(h.text('temperature-peaks'), /not current sensor hotspots/); assert.equal(h.inspect('globalThis.pwned'), undefined);
  assert.match(html, /SMC grouping is heuristic/);
});

await check('old backends and missing or VM sensor snapshots retain valid mean temperatures without invented sensor detail', async () => {
  const h = boot({ expanded: true }); await h.answer(sample());
  assert.equal(h.text('cooling-cpu-temp'), '45.0 °C'); assert.equal(h.elements.get('temperature-table').hidden, true);
  assert.match(h.text('temperature-groups'), /unavailable/i);
  await nextPoll(h); await h.answer(sample({ sequence: 2, temperatures: [], data: metrics({ temp: { cpu_temp_avg: null, gpu_temp_avg: 0 } }) }));
  assert.equal(h.text('cooling-cpu-temp'), 'Unavailable'); assert.equal(h.text('cooling-gpu-temp'), '0.0 °C');
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: metrics({ soc: { vm: true } }), temperatures: [{ name: 'fake', group: 'cpu', source: 'smc', celsius: 70 }] }));
  assert.equal(h.text('cooling-cpu-temp'), 'Unavailable'); assert.equal(h.elements.get('temperature-table').hidden, true);
  assert.equal(h.elements.get('temperature-rows').children.length, 0); assert.equal(h.frames.size, 0);
});

await check('temperature charts redraw after expansion, break gaps and keep a bounded history of mean peaks', async () => {
  const h = boot(), canvas = h.elements.get('temperature-chart');
  await h.answer(sample()); assert.equal(canvas.width, undefined, 'A hidden chart must not receive a fake size');
  await nextPoll(h); await h.answer(sample({ sequence: 2, data: metrics({ temp: { cpu_temp_avg: 50, gpu_temp_avg: 48 } }) }));
  await nextPoll(h); await h.answer(sample({ sequence: 3, data: metrics({ temp: { cpu_temp_avg: null, gpu_temp_avg: null } }) }));
  await nextPoll(h); await h.answer(sample({ sequence: 4, data: metrics({ temp: { cpu_temp_avg: 47, gpu_temp_avg: 49 } }) }));
  await h.click('cooling-toggle'); assert.equal(canvas.width, 600);
  for (const trace of canvas.context.paths) assert.deepEqual(trace.points.map((point) => point[0]), ['moveTo', 'lineTo', 'moveTo']);
  assert.match(h.text('temperature-range'), /40–50 °C/); assert.match(h.text('temperature-peaks'), /CPU 50.0 °C \/ GPU 49.0 °C/);
  assert.equal(h.text('power-range'), '0–20 W', 'Temperature values never scale the power chart');
  await h.click('cooling-toggle'); canvas._width = 170;
  for (let sequence = 5; sequence < 68; sequence++) {
    await nextPoll(h); await h.answer(sample({ sequence, data: metrics({ temp: { cpu_temp_avg: 30, gpu_temp_avg: 31 } }) }));
  }
  await h.click('cooling-toggle'); assert.equal(canvas.width, 340);
  assert.equal(h.state().history.length, 60); assert.match(h.text('temperature-peaks'), /CPU 30.0 °C \/ GPU 31.0 °C/);
  assert.equal(canvas.context.paths[0].points.length, 60);
});

console.log(`${tests.length} dashboard checks passed.`);
