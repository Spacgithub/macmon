Done: 2026-10-14, PR #4

# Refine the terminal dashboard

Continue the person's requested visual refinement in personal-fork PR #4. Make the terminal
dashboard feel precise and modern while retaining Network Monitoring and upstream compatibility.
All GitHub changes target `Spacgithub/macmon`; the original repository remains read-only.

## Behavior

- Separate the brand, hardware name and quieter specifications; narrow windows drop complete
  header components before losing important information.
- Emphasize metric numbers and keep units secondary, preserving value text, precision and
  warning colors. Give the newest graph sample a clear emphasis over older history.
- Align network and process tables with consistent inset where space permits. Fixed numeric
  widths and whole-column fallbacks stay intact; wide network tables use the available width.
- Emphasize process names and the current sort values while reducing PID/user metadata weight.
- Keep ANSI theme colors, geometry, controls, Gauge/VM behavior, configuration and output APIs.
  Sampling, history buffers, redraw triggers and dependencies remain unchanged.
- Validate native buffers, rootless terminal operation and bounded render cost, then update
  the existing PR and the actual local command with a recoverable previous executable.

## Tasks

- [x] Review the existing UI and select bounded visual refinements.
- [x] Refine headers, metric hierarchy, chart emphasis and table alignment.
- [x] Validate widths, warning/selection styles, native previews and resource use.
- [x] Complete independent review and update the personal-fork PR.
- [x] Install the checked executable and verify the actual command entry point.

## Result

Refined brand/hardware hierarchy, numeric emphasis, secondary units, newest/history chart
contrast and table alignment using terminal ANSI colors. Warning colors, Gauge, VM layout,
Network Monitoring, output APIs, configuration, controls and sampling remain compatible.
This refinement adds 79 Rust production lines; the combined PR #4 adds 175, tests excluded.
PR #4 is ready for the person's review and squash merge.

Validation passed: `make check`, 204 Rust tests and 14 sync fixture groups, native Sparkline/
Gauge previews at compact and wide sizes with sample light/dark palettes, release build,
independent source/UI reviews, final Codex CLI review and GitHub CI. Rootless M2 Max/macOS
15.6.1 runs at 80x24, 60x18 and 120x40 restored terminal modes and preserved saved settings.
Real VM operation and an interactive Apple Terminal theme were not tested.

Separate release-target render checks against the preceding UI measured 88.73→91.20,
138.60→141.43 and 210.08→215.30 microseconds/frame at 80x24, 120x40 and 200x50. Three short
paired rootless PTY runs measured median process CPU 1.86%→1.87%; whole-machine power was
not measured. No new animation, sampler, history buffer or dependency was added.

The actual local `macmon` command now starts the checked refinement in the person's
`~/.local/bin/macmon`. Installed-command JSON retained 21 interfaces; the brand header,
directional network columns, resize and quit passed in a real PTY. The previous executable
is recoverable under `~/.local/state/macmon/backups/`, and the Homebrew Cellar is intact.
