Done: 2026-10-14, PR #13

# Dashboard processes and temperatures

## Behavior and decisions

- Retain overall CPU/GPU activity and add a compact Processes table with process name/PID,
  CPU, GPU and memory, sortable by usage. Reuse the app-layer ProcSampler and its existing
  accounting; HTTP requests only read completed snapshots. Missing rates remain unavailable.
- Show named CPU and GPU temperatures with clear degree-Celsius units and explicit unknown
  values, keeping the existing responsive dashboard and compact Cooling cards.
- Preserve old JSON, Prometheus, library API and terminal configuration. Add no dependencies.
- Keep this increment within about 200 production lines; publish through the personal fork
  after review, with the dashboard PR as its temporary base. Do not merge any PR.

## Tasks

- [x] Publish real process snapshots without request-thread sampling.
- [x] Add sortable processes and prominent temperature readings.
- [x] Verify missing data, accounting and desktop/narrow layouts.
- [x] Run complete checks and review the personal-fork PR.
- [x] Refresh and verify the installed command with a working backup.

## Result

The dashboard shows real sortable process snapshots and named Celsius temperatures,
with unavailable rates kept explicit. Unit, frontend, rootless hardware and responsive
browser checks passed; the combined installed preview was verified with a retained backup.
