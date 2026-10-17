# Core summaries, useful details and dashboard layout

## Behavior and decisions

- Compact views answer the current state; expanded views add measurements, breakdowns and
  history. Keep independent and global controls, keyboard access and released preferences.
- CPU overview shows overall reported activity and temperature. Details add E/P/S tier
  activity, reported/installed core counts, real die/core rows and activity history, with
  source-supported frequency-weighted residency, frequency and power fields.
- GPU overview shows reported activity and temperature. Details add activity versus
  frequency-weighted residency, active-time mean frequency, supported frequency range,
  reported GPU core count, existing power and history. Do not invent GPU per-core load or FPS.
- Tick fallbacks and VMs retain valid activity but do not masquerade as measured frequency
  or frequency-weighted data. Missing cores and measurements are not filled with false zeros.
- Network gains an overview of one explicitly named automatic observation interface,
  independent receive/transmit rates, observed interface count, traffic-active count and
  rates-pending count. Choose the busiest valid single interface with stable name tie breaks;
  never add VPN, bridge and physical-interface traffic into a supposed whole-system total.
- Distinguish no sample, unavailable collection, no reported interfaces, pending rates,
  actual zero-rate idle and observed traffic. These are monitoring states, not Internet
  connectivity claims; activity and rates-pending counts can overlap.
- Expanded Network retains every reported interface and cumulative counters with complete
  units. Existing API, library metrics, pipe and Prometheus stay unchanged; add no dependency.
- Web keeps compact summary cards balanced and places independently opened CPU/GPU details
  in a dedicated full-width area. Expanded lists receive enough width, responsive cards
  collapse cleanly, and changing one panel does not leave large blank columns elsewhere.
  Preserve memory pressure, Swap activity, Power, Disk health/history and compact fans.
- Terminal keeps all core summaries usable at 80x24, puts tier/core breakdowns in scrollable
  detail/fullscreen views and applies the Network overview consistently to compact/advanced
  layouts. Preserve process focus/filter/selection, existing settings and fan placements.
- Deliver two dependent feature PRs after the existing disk stack: Web then Terminal.
  Leave the installed program and formal service on merged main until these PRs are merged.

## Tasks

- [x] Add meaningful browser CPU/GPU details and honest Network overview semantics.
- [x] Rebalance browser compact/expanded layout with responsive and persistence coverage.
- [x] Add matching Terminal Compute and Network summaries/details with layout regressions.
- [ ] Run focused and full checks, independent reviews, and real browser/private-PTY checks;
  retain all disk functionality and verify the user's installed service and settings.
