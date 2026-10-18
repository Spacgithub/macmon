Done: 2026-10-18, PR #35

# Resource efficiency in Terminal and Web

## Behavior and decisions

- Deliver three dependent feature PRs in Spacgithub/macmon: a shared sampling source,
  demand-driven process collection, then adjustable animation and integrated cost validation.
- Keep the normal monitor rootless. Terminal and the HTTP backend consume one per-user
  sampler through a private, bounded, versioned local transport. An exclusive owner lock
  precedes sampler creation, including concurrent startup and recovery.
- Do not create a persistent launchd job for a standalone Terminal run. A temporary collector
  exits after its last consumer disconnects; an existing web backend keeps its normal lease
  until `web stop`. Reconnect safely after failure without running a duplicate local sampler.
- Use one effective system sampling interval. Explicit interval changes from either interface
  are reflected by all consumers; do not overwrite another interface's saved preference.
  Preserve a run-only CLI override, existing web controls and honest freshness during recovery.
- Keep independent `pipe` behavior, legacy JSON/Prometheus, library APIs, all existing metric
  sources, dashboard privacy rules and released configuration fields. Add no dependency.
- Separate full process snapshots from system-metric cadence. Collect the complete PID set
  for accurate rankings and application totals: summaries at a lower frequency, detailed
  lists faster only while requested, and no collection when no consumer needs processes.
  Respect slower requested system intervals and use actual elapsed time for every rate.
- Combine bounded expiring process-demand leases across Terminal and browser clients.
  Hidden/inactive browser views release detailed demand. Retain compatibility for older
  dashboard clients and show actual process age; never label retained data as a new reading.
- Application expansion remains independent and all helpers remain reachable. Only an
  explicitly selected real PID may be acted on; PID reuse and source changes invalidate
  stale results and rate baselines.
- Read selected-PID file/socket details only while its overlay is open, with a lower refresh
  rate and explicit refresh. Keep slow work away from interactive rendering and reject late
  results from a closed/replaced target; never scan files for every group member.
- Offer 15/30/60 FPS in both interfaces, initially 30 FPS, while retaining the existing motion
  switch. Change actual wakeup/draw scheduling, independently of sample cadence; preserve
  pause, focus/visibility, reduced-motion and terminal cleanup behavior.
- Validate on this Mac using release builds, matching dimensions, intervals, process views,
  animation settings and HTTP/browser workloads. Measure CPU-time deltas and sampled memory
  over repeated bounded runs, include the collector and completed helper CPU, and report
  noise and any memory tradeoff rather than assuming every metric improves.
- Keep settings and the formal installed program/backend on merged main while the feature
  PRs are under review. Use private runtime namespaces, test ports and preference-free PTYs
  for candidate validation; keep reproducible measurement methodology with the PR results.

## Tasks

- [x] Record an optimization baseline and define matched before/after scenarios.
- [x] Implement and verify shared sampling, cadence, safe transport and collector lifetime.
- [x] Implement and verify demand-driven process snapshots and selected-PID detail collection.
- [x] Add Terminal/Web frame-rate controls and verify real scheduling at each rate.
- [x] Complete full checks, independent reviews, real hardware/browser regression and
      repeated CPU/memory comparisons; retain all existing monitoring and compatibility.

## Result

Delivered in dependent PRs #33 → #34 → #35: one temporary rootless collector,
5-second/1-second demand-driven full process snapshots and asynchronous details for the selected
PID, plus saved 15/30/60-FPS controls. Legacy outputs, library-only builds,
preferences and existing monitoring/grouping remain supported without dependencies.

Full checks, independent reviews, real hardware/Terminal/browser regression and
three matched 30-second rounds per workload passed. CPU medians fell 24.5–65.9%;
RSS rose about 19–28 MiB including the collector. See [measurement method and
results](../../resource-efficiency.md) for ranges, footprint and runtime coverage.
The installed program and primary main remain on merged main pending review.
