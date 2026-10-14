Done: 2026-10-14, PR #5

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
- [x] Validate configuration compatibility, VM/Gauge behavior, widths, keys and terminal exit.
- [x] Measure static/motion CPU, RSS and redraw frequency with stated workload limits.
- [x] Complete integrated reviews, required checks and the personal-fork feature PR.
- [x] Refresh and verify the installed command and fresh zsh entry point.

## Result

The native wide console and optional Warp palette are implemented, with compact/VM fallbacks.
Reported-RPM fan motion is bounded, saved with `a`, and pauses without changing orientation.
Network Monitoring and existing output/configuration compatibility are preserved. The personal
fork PR passes checks; the installed command matches the tested release build and is verified
from fresh zsh and real rootless network/TUI runs, with the previous executable retained.
