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
- [ ] Complete independent review and update the personal-fork PR.
- [ ] Install the checked executable and verify the actual command entry point.
