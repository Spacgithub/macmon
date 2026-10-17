# Disk activity and SSD health

## Behavior and decisions

- Add rootless per-physical-device read/write byte rates and IOPS from the documented
  IOBlockStorageDriver cumulative counters, with device identity and monotonic elapsed time.
  Exclude virtual/APFS layers and empty card readers; never sum volumes as physical devices.
- Keep counters and rates separate. Missing counters, first observations, failed reads,
  reconnects and resets remain unavailable until a valid baseline exists; idle rates may be zero.
  The I/O counters describe host block traffic, not NAND writes or lifetime wear.
- Read supported NVMe SMART data through the SDK IONVMeSMARTInterface without sudo or added
  dependencies. Report critical warnings, percentage used, spare capacity, composite Celsius
  temperature, lifetime host reads/writes, power-on hours and media errors only when available.
- Preserve vendor-reported percentage used, including values above 100. Do not predict a
  failure date or treat 100 minus this number as a guaranteed remaining life. Convert NVMe
  data units as 512,000 bytes and preserve wide counters without arithmetic/JSON precision loss.
- Read health infrequently, expose its freshness, release native resources and distinguish
  unsupported, unavailable and stale data from a successful zero-valued health report.
  External bridges, SATA devices and VMs may have incomplete health information.
- Publish additive sampler accessors and same-sample top-level disk data in `/api/sample`.
  Preserve existing public metric structs, pipe/legacy JSON, Prometheus and configuration.
- Add a concise Terminal Disk summary with keyboard-accessible device details and a matching
  browser card. Reuse independent/global expansion, complete units and responsive layouts.
  Keep Memory, Network, Cooling and Processes usable at 80x24 and expose full lists on demand.
- Deliver three dependent PRs: sampling/health/API, Terminal integration, browser integration.
  Each PR includes focused regressions; the final PR completes this shared plan. The person
  approved the complete three-PR scope and expanded feature budgets on 2026-10-17.

## Tasks

- [x] Add physical-device enumeration, validated counters and reset-safe rate sampling.
- [x] Add bounded native NVMe health collection, freshness, identity mapping and failure fallbacks.
- [x] Publish compatible library/API disk snapshots with precision and legacy-output tests.
- [x] Add Terminal summary/details, controls and narrow/compact/expanded layout coverage.
- [x] Add browser card/history/details with persisted expansion and missing/stale-data coverage.
- [ ] Run full checks and independent reviews, and compare rootless hardware readings with
  independent I/O and SMART probes; verify Terminal/Web behavior without changing user settings.
