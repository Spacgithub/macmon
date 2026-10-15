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
      style: {}, dataset: {}, hidden: false, checked: false, _text: '', clientWidth: 300, clientHeight: 74 });
  }
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
    return { setTransform() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {} };
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

function boot({ reduced = false } = {}) {
  let now = 0, nextId = 1, activeFetches = 0, maximumFetches = 0;
  const timers = new Map(), frames = new Map(), requests = [], elements = new Map();
  for (const match of html.matchAll(/<([a-z][\w-]*)\b([^>]*\bid="([^"]+)"[^>]*)>([^<]*)/gi)) {
    const element = new Element(match[1], /\bclass="([^"]*)"/.exec(match[2])?.[1] || '');
    element._text = match[4].replaceAll('&mdash;', '—');
    element.hidden = /\bhidden\b/.test(match[2]);
    const section = new Element(), parent = new Element();
    section.append(parent); parent.append(element);
    elements.set(match[3], element);
  }
  elements.get('fan-template').content = fanTemplate();
  const document = new Target();
  Object.assign(document, { hidden: false, documentElement: new Element('html'),
    getElementById(id) { assert.ok(elements.has(id), `Unknown DOM id: ${id}`); return elements.get(id); },
    createElement(tag) { return new Element(tag); } });
  const media = new Target(); media.matches = reduced;
  const timer = (callback, delay = 0) => {
    const id = nextId++;
    timers.set(id, { at: now + Math.max(0, Number(delay)), callback });
    return id;
  };
  const fetch = (url, options) => {
    assert.equal(url, '/api/sample');
    assert.equal(options.cache, 'no-store');
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
    performance: { now: () => now }, fetch, matchMedia: () => media,
    getComputedStyle: () => ({ getPropertyValue: () => '#72d8e8' }),
    ResizeObserver: class { observe() {} },
    setTimeout: timer, clearTimeout: (id) => timers.delete(id),
    requestAnimationFrame(callback) { const id = nextId++; frames.set(id, callback); return id; },
    cancelAnimationFrame: (id) => frames.delete(id) });
  new vm.Script(source, { filename: sourcePath }).runInContext(context, { timeout: 1000 });
  const inspect = (expression) => vm.runInContext(expression, context, { timeout: 1000 });
  const pending = () => requests.find((request) => !request.done);
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
    elements, document, media, frames, requests, inspect, pending,
    state: () => inspect('state'),
    text: (id) => elements.get(id).textContent,
    status: () => elements.get('status').dataset.state,
    now: () => now,
    maximumFetches: () => maximumFetches,
    async answer(payload, status = 200) { assert.ok(pending(), 'No pending real poll'); pending().answer(payload, status); await drainMicrotasks(); },
    async fail() { assert.ok(pending()); pending().fail(); await drainMicrotasks(); },
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
    gpu_freq_mhz: 900, cpu_tiers: [{ label: 'E', freq_mhz: 1200 }, { label: 'P', freq_mhz: 2400 }],
    memory: { ram_usage: 8 * 1024 ** 3, ram_total: 32 * 1024 ** 3, swap_usage: 0 },
    temp: { cpu_temp_avg: 45, gpu_temp_avg: 43 },
    network: [{ name: 'en0', rx_bytes_per_sec: 1000, tx_bytes_per_sec: 500, rx_bytes: 4096, tx_bytes: 2048 }],
    fans: [{ name: 'fan0', rpm: 1800, max_rpm: 6800 }, { name: 'fan1', rpm: 2400 }], ...overrides };
}
function sample({ sequence = 1, sampled_at = '2026-10-14T12:00:00Z', age_ms = 0, interval_ms = 1000, data = metrics() } = {}) {
  return { sequence, sampled_at, age_ms, interval_ms, metrics: data };
}
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
    fans: [{ name: 'not-a-real-vm-fan', rpm: 9999 }], cpu_power: 99, gpu_power: 88, gpu_freq_mhz: 777 } }));
  assert.equal(h.text('cpu-value'), '—');
  assert.equal(h.text('gpu-value'), '—');
  assert.equal(h.text('ram-value'), '—');
  assert.ok(!h.text('cpu-detail').includes('99'));
  assert.ok(!h.text('gpu-detail').includes('88') && !h.text('gpu-detail').includes('777'));
  assert.equal(h.elements.get('network-table').hidden, true);
  assert.match(h.text('network-empty'), /unavailable/i);
  assert.equal(h.state().fans.size, 0);
  assert.equal(h.frames.size, 0);
  await nextPoll(h);
  await h.answer(sample({ sequence: 2, data: {} }));
  assert.match(h.text('network-empty'), /unavailable/i, 'An omitted network field must not claim zero interfaces');
  assert.equal(h.state().fans.size, 0);
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
  const h = boot(); await h.answer(sample()); await animate(h);
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
  const h = boot(); await h.answer(sample({ sequence: 4 }));
  await nextPoll(h); await h.answer(sample({ sequence: 4, age_ms: 1000 }));
  assert.equal(h.state().history.length, 1);
  await nextPoll(h); await h.answer(sample({ sequence: 4, sampled_at: '2026-10-14T12:05:00Z' }));
  assert.equal(h.state().history.length, 2);
  for (let sequence = 5; sequence < 70; sequence++) {
    await nextPoll(h); await h.answer(sample({ sequence, sampled_at: '2026-10-14T12:05:00Z' }));
  }
  assert.equal(h.state().history.length, 60);
  assert.equal(h.maximumFetches(), 1);
});

await check('503 waiting, offline and stale states recover without erasing the last values', async () => {
  const h = boot(); await h.answer(null, 503);
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
  const h = boot(); await h.answer(sample()); await animate(h, 10);
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
  const initial = boot({ reduced: true }); await initial.answer(sample());
  assert.equal(initial.frames.size, 0); assert.equal(initial.elements.get('motion').checked, false);
});

await check('hidden tabs cancel animation and polling, then refresh on return', async () => {
  const h = boot(); await h.answer(sample()); await animate(h, 5);
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

console.log(`${tests.length} dashboard checks passed.`);
