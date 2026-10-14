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
- Use chunked inline RGBA images in Warp and private file images in other supported terminals.
  Query the selected transport and format, keep query acknowledgments enabled and suppress
  routine frame replies. Complete each image before another terminal write can interleave.
- Keep rootless metric sources, Network Monitoring, JSON, Prometheus, the library API and
  dependency versions compatible. Deliver rendering, terminal capability/ownership and UI
  integration in one feature PR, with the person's approved exception to the production-line
  budget. Keep the full production change measured against fork main.
- Refresh the installed executable only after integrated checks pass, retain a recoverable
  previous executable and verify fresh-zsh command resolution and real rootless operation.

## Tasks

- [x] Port and test the rasterizer and independent 60 FPS rotor clock.
- [x] Implement bounded terminal capability detection and safe reply handling.
- [x] Implement owned image transmission and complete cleanup paths.
- [x] Handle Warp builds without local-file image support using bounded inline packets.
- [x] Integrate real fan identities, adaptive cooling panels and manual/focus/visibility gating.
- [x] Validate output/config compatibility, resize, zero/missing RPM, terminal recovery and
      static/animated CPU, RSS and submitted-frame rate; distinguish actual Warp display checks.
- [ ] Verify visible fan images and terminal restoration in the person's actual Warp session.
- [ ] Complete integrated reviews, required checks and one feature PR in the personal fork.
- [x] Refresh and verify the installed command with the previous binary retained.
