# Deep-space console and bounded fan motion

Implement the person's selected first concept from the side conversation as a native Rust TUI,
with optional fan motion. All commits and PRs target `Spacgithub/macmon`; upstream stays read-only.

## Behavior

- Wide windows group Compute on the left and Memory, Power and Temperature on the right, above
  Network and Processes. Compact windows and VMs retain complete adaptive metric displays.
- Use the existing ANSI metric accents and red load warnings, layered hardware information and
  aligned tables. Supply an optional Warp theme/profile guide for the deep blue background.
- Animate a small fan glyph from real reported RPM, at approximately 1–4 frame changes per
  second using the existing 250 ms heartbeat. Zero RPM is stationary; absent data stays absent.
- `a` toggles motion and saves a compatible defaulted boolean. Preserve released field names,
  `Sparkline`/`Gauge`, interval and other settings. Turning motion off stops animation redraws.
- Pause motion on focus loss and while its indicator is hidden; resume on focus gain. Sampling,
  network counters, process collection, library/JSON/Prometheus APIs and dependencies stay intact.
- Compare static and motion CPU/RSS and frame counts, distinguishing real idle hardware from
  labeled moving-fan fixtures. Validate widths, controls and terminal restoration, publish a fork
  feature PR, then refresh the actual installed command with a recoverable previous executable.

## Tasks

- [x] Adapt the selected layout and provide the companion Warp theme instructions.
- [x] Implement bounded fan motion, the saved switch and visibility/focus gating.
- [ ] Validate configuration compatibility, VM/Gauge behavior, widths, keys and terminal exit.
- [ ] Measure static/motion CPU, RSS and redraw frequency with stated workload limits.
- [ ] Complete integrated reviews, required checks and the personal-fork feature PR.
- [ ] Refresh and verify the installed command and fresh zsh entry point.
