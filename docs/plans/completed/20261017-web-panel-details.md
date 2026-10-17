Done: 2026-10-17, PR #20

# Browser overview and detail controls

## Behavior and decisions

- Depend on the temperature-details feature and keep the existing local dashboard style.
- Default to concise live summaries with independent expand/collapse controls for CPU,
  GPU, memory, power, Network, Cooling/temperature and Processes; add expand-all/collapse-all.
- Keep three busiest named interfaces and five sorted processes in summaries; details
  expose every row with scrolling and preserve sorting, freshness and focus.
- Show CPU/GPU averages, current sensor range/count and named readings with source, plus
  temperature history and clearly distinguished historical average peaks.
- Preserve expansion preferences in browser-local storage with safe fallback when blocked.
- Collapsed panels keep summaries fresh; hidden charts redraw on expansion, and collapsed
  Cooling does not schedule fan animation. Do not change backend sampling or TUI settings.
- The person approved the complete three-PR scope and expanded feature budgets.

## Tasks

- [x] Add accessible per-panel and global overview/detail controls with persistence.
- [x] Add temperature summaries, trends and real sensor details.
- [x] Verify lifecycle, missing/stale data, storage fallback and responsive browser layouts.
- [x] Run checks, tests and independent review.

## Result

All seven panels support independent and global detail controls with browser-local
preferences. Real sensor ranges, hotspots and mean-temperature history are available in
Cooling. Rootless browser checks on M2 Max / macOS 15.6.1, responsive layouts, live-scroll
retention, full checks and independent review passed.
