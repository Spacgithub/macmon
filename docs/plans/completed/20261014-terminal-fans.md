Done: 2026-10-14, PR #11

# Adaptive cooling cards across terminals

## Behavior and decisions

- Size complete fans continuously from the available physical cell dimensions, bounded to
  compact diameters. Keep readable CPU, GPU, memory, Network and process panels; use RPM
  indicators when a complete fan cannot fit, and restore it as space returns.
- Use verified native images in capable terminals and a complete ANSI Braille fan in
  Apple Terminal and other text terminals. Share the seven-blade raster, real RPM and rotor
  state, without sending image probes to unsupported terminals.
- Keep the independent 60 FPS target, manual animation setting, stopped-fan behavior and
  focus/help visibility gates. Reuse static UI and raster storage for animation frames.
- Preserve configuration, library, JSON and Prometheus compatibility. Update personal-fork
  PR #11 and the installed command, retaining the previous working binary and settings.

## Tasks

- [x] Implement compact continuous layout and physical cell aspect handling.
- [x] Add the complete text fan and separate graphics support from cell measurement.
- [x] Verify layouts, both rendering paths, animation gates and terminal cleanup.
- [x] Run complete checks and review the integrated change in PR #11.
- [x] Refresh and verify the installed command with the previous binary retained.

## Result

Compact fan diameters now adapt continuously to measured cell pixels. Native images and
complete ANSI Braille fans share geometry, actual RPM and the independent frame clock.
Small windows preserve Network and usable process rows; missing pixel geometry after resize
uses a bounded text fallback. Cached animation, resizing, protocol fallback and cleanup
passed integrated tests and private PTY checks. The installed command was refreshed, backed
up and checked against rootless hardware readings with settings preserved.
