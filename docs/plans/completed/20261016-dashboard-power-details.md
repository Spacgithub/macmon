Done: 2026-10-16, PR #17

# Dashboard power details

## Behavior and decisions

- Add a compact responsive Power panel using the existing completed metrics. Show CPU,
  GPU, ANE, DRAM and GPU SRAM in watts, with CPU+GPU+ANE subtotal and system estimate
  clearly distinguished. Keep the current Network, terminal and browser features.
- Plot up to 60 distinct completed samples for the subtotal and system estimate on a
  shared watt scale. Never connect missing samples or combine the two totals.
- System estimate is available only when positive; missing, invalid and VM values stay
  unavailable. Reported component zeros remain explicit, with a concise explanation that
  the current API cannot distinguish all unsupported counters from zero consumption.
- Preserve JSON, Prometheus, library API, configuration and sampling. Add no dependencies
  and keep production changes below about 200 lines. Publish a separate personal-fork PR.

## Tasks

- [x] Add honest component readings, totals and watt history.
- [x] Verify missing/zero/VM data and responsive layouts.
- [x] Run complete checks and independent review.
- [x] Publish the personal-fork PR and verify the installed preview with a retained backup.

## Result

The browser now shows separate system and CPU+GPU+ANE power histories and five component
readings, preserving tiny positive values in mW and explicit unavailable states. Complete
checks, independent review, rootless hardware and desktop/narrow browser checks passed;
the installed preview was refreshed with a retained backup and unchanged settings.
