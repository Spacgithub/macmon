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

- [ ] Add accessible per-panel and global overview/detail controls with persistence.
- [ ] Add temperature summaries, trends and real sensor details.
- [ ] Verify lifecycle, missing/stale data, storage fallback and responsive browser layouts.
- [ ] Run checks, tests and independent review.
