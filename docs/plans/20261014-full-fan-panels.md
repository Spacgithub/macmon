# Full graphical fan panels

Implement the selected deep-space cooling panels in the native terminal UI. All branches and PRs
target `Spacgithub/macmon`; `vladkens/macmon` remains a read-only upstream.

## Behavior and decisions

- Show one complete circular, seven-blade fan for each reported fan, with a stationary rim and
  center shaft, curved blades and metallic shading. Prefer terminal image rendering at 180–240
  pixels where the window and terminal capabilities permit it; retain the compact RPM display
  on unsupported terminals, small windows and unavailable hardware.
- Use reported fan names and RPM without assigning CPU/GPU roles, inventing a second fan or
  substituting synthetic readings. Keep each rotor's angle across metric reorderings. Zero or
  missing RPM stops that rotor immediately. Visual rotation is an illustrative, smoothed speed.
- Target 60 image updates per second independently of the metrics and process sampling clocks.
  Skip missed frames instead of accumulating a backlog. Animate only visible, focused panels.
- Preserve the saved manual `a` switch and all released settings, including the current sampling
  interval. Receiving metrics, detecting terminal capabilities or resuming focus cannot change
  the person's saved animation choice. No fan hardware controls are added.
- Derive image shades from queried terminal colors through `tui/theme.rs`. Capability queries,
  delayed replies and fragmented replies cannot act as keyboard shortcuts. Image ownership is
  limited to this macmon process; hide, resize, error, panic and exit remove its own images.
- Keep rootless metric sources, Network Monitoring, JSON, Prometheus, the library API and
  dependency versions compatible. Organize rendering, terminal capability/ownership and UI
  integration as small reviewable PRs, with each incremental production change measured.
- Refresh the installed executable only after integrated checks pass, retain a recoverable
  previous executable and verify fresh-zsh command resolution and real rootless operation.

## Tasks

- [x] Port and test the rasterizer and independent 60 FPS rotor clock.
- [x] Implement bounded terminal capability detection and safe reply handling.
- [x] Implement owned image transmission and complete cleanup paths.
- [x] Integrate real fan identities, adaptive cooling panels and manual/focus/visibility gating.
- [ ] Validate output/config compatibility, resize, zero/missing RPM, terminal recovery and
      static/animated CPU, RSS and submitted-frame rate; distinguish actual Warp display checks.
- [ ] Complete integrated reviews, required checks and feature PRs in the personal fork.
- [ ] Refresh and verify the installed command with the previous binary retained.
