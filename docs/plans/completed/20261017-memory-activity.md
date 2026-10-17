Done: 2026-10-17, PR #22

# Memory pressure and swap activity

## Behavior and decisions

- Read the operating system's memory-pressure level, separately from RAM usage percentage.
  Unknown or unavailable levels remain unavailable; preserve older-system fallbacks.
- Measure independent swap-in/out rates from cumulative VM swap-page counters, actual page
  size and elapsed monotonic time. These are page-equivalent bytes, not physical SSD I/O.
- First samples, missing counters, resets, zero intervals and page-size changes must not
  invent zero rates or spikes. Failed metric acquisitions must not leave current-looking data.
- Expose a read-only sampler accessor and a same-sample top-level `memory_activity` object in
  `/api/sample`; preserve public metric structs, legacy pipe/JSON/Prometheus and config formats.
- Show concise pressure and swap activity in terminal/browser Memory summaries, with complete
  details available through the existing controls. Keep all units readable in small windows.
- Add no Cargo dependencies. The person approved a complete independent PR and expanded the
  production-line budget for this memory feature on 2026-10-17.

## Tasks

- [x] Add rootless pressure/counter sources, safe rate sampling and focused regression tests.
- [x] Publish compatible library and HTTP snapshots and verify failure/freshness semantics.
- [x] Add terminal and browser summaries/details with missing-data and layout coverage.
- [x] Run full checks, independent review and real rootless comparison with system counters.

## Result

Added same-sample OS memory pressure and safe independent swap-in/out page-equivalent rates,
with compatible library/HTTP accessors and terminal/browser summaries and details. Focused
regressions cover missing sources, counter/time resets and compact two-fan layouts. Full
checks, independent review and rootless M2 Max runtime comparisons passed.
