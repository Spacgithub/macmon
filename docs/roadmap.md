# Roadmap

Work on one item at a time, each through its own PR. When an item is taken, its plan goes to
`docs/plans/yyyymmdd-<name>.md` and is linked here. Tick an item in the PR that finishes it.

Process data beyond CPU% and memory comes from libproc, which reads only the user's own
processes without root (all of them under sudo). Other users' processes show `-` there, as
POWER does today.

- [x] **1. Kill the selected process** (#89). `k` → popup: Terminate / Force kill / Cancel; the
      start time is checked before the signal; launchd and macmon are refused.
      Plan: [docs/plans/completed/20261010-kill-process.md](plans/completed/20261010-kill-process.md).
- [x] **2. Show in Finder.** `e` on the selected process runs `open -R` on its executable.
      Works for every process (the path is readable for all users).
      Plan: [docs/plans/completed/20261011-process-info.md](plans/completed/20261011-process-info.md).
- [x] **3. Open files and sockets of a process.** `i` on a process opens its list, as Activity
      Monitor's "Open Files and Ports" and Sloth show it: working directory, files and sockets
      (`local → remote` address), read as `lsof` does: the working directory with
      `proc_pidinfo(PROC_PIDVNODEPATHINFO)`, the descriptors with `PROC_PIDLISTFDS` and each one with
      `proc_pidfdinfo`. The list has a filter; `e` on a file reveals it in Finder (`open -R`), Esc
      goes back to the processes.
      Plan: [docs/plans/completed/20261011-process-info.md](plans/completed/20261011-process-info.md).
- [ ] **4. A C library for other UIs.** Ship macmon as a dylib with a C header, so Swift, Python
      or C apps sample metrics without parsing `macmon pipe`. Builds on #59 and
      [homm/macmon-bindings](https://github.com/homm/macmon-bindings) (Python and Swift today,
      published from a fork). Decision point before the plan: a C API of plain structs, as in
      #59, or a small one that returns the `pipe` JSON (sampler new / next / free, SoC info,
      last error), which keeps the ABI stable while metrics change. Release packaging: the dylib,
      the header and an `XCFramework`; bindings stay outside this repository.
- [ ] **5. More process columns.** Candidates: CPU TIME (total, all processes: already sampled
      from `rusage` and `ps`), THREADS (`proc_taskinfo`), IDLE WAKE/s (`rusage`
      `ri_pkg_idle_wkups`), FDS (open file descriptors), DISK R/W/s (`rusage` disk bytes), PEAK
      MEM (`ri_lifetime_max_phys_footprint`). Fixed set, no configuration: new columns go into
      `DROP_ORDER`, so a narrow window drops them first. Decision point before the plan: which of
      them earn a place in an 80-column window.
- [ ] **6. Column picker.** Only if item 5 leaves too many columns: an overlay with checkboxes
      (like htop's F2) saved in `~/.config/macmon.json` under a new field. Released versions
      ignore the field but drop it when they save, so a missing field means the default columns.
- [ ] **7. A clearer info overlay.** Two PRs. First, the files list: `exe` and `cwd` first, then
      files sorted by directory, a type and open mode (`r-`, `-w`, `rw`) on every row, duplicates
      as `×N`, devices, pipes, kqueues and unnamed sockets left out of the list and counted in a
      line above it (`116 fds  8 files  3 dev  96 pipes …`). Second, a process summary line
      above the counts (cpu, mem, power, threads, uptime, parent) and presets on `s` / `S` (`all`,
      `files`, `network`, `home`, `not home`) saved in `~/.config/macmon.json`.
      Plan (first PR): [docs/plans/20261011-grouped-files.md](plans/20261011-grouped-files.md).
- [ ] **8. macmon's own cost, before the release.** Measure what the TUI, `pipe` and `serve` cost
      at 1000 ms and 250 ms: CPU time per minute and power (macmon's own POWER, `powermetrics`
      task energy, Activity Monitor's Energy Impact), with the process list shown and hidden.
      Profile the hot paths (sampling, IORegistry GPU reads, `ps`, rendering) and fix what is
      cheap to fix; report before / after numbers in the PR.
- [x] **9. Scrolling in the lists.** A scrollbar on the right border of the process list and the
      info overlay (ratatui's `Scrollbar`: `↑` / `↓` at the ends, a thumb between, as btop
      draws it), ↑ / ↓ wrapping around at the ends (not the wheel, PgUp / PgDn, Home / End), and
      `c` copying the selected path (the executable in the list, the row's full text in the info
      overlay); `? | q back` on the overlay's bottom border.
      Plan: [docs/plans/completed/20261012-list-scroll.md](plans/completed/20261012-list-scroll.md).
- [ ] **10. Pause and filter off.** A key that freezes the process list and the info overlay, so
      a row can be read and selected without the list jumping (Sloth has it). The metrics keep
      updating; the title shows that the list is paused. And `f` turning the filter off and on
      again without retyping it, to look outside it for a moment: the filter text stays, faint,
      while it is off.
- [ ] **11. Who has a file open.** A view of the open files of all processes with the filter,
      to find who reads or writes a file or holds a port (Sloth's main window, `lsof <path>`).
      Without root libproc reads only the user's own processes; other users' processes are
      counted as not readable. Decision point before the plan: a separate tab or a mode of the
      process list.
- [ ] **12. Run in a VM.** macmon in a macOS VM (Apple Virtualization, UTM), where IOReport,
      the SMC and the frequency tables are missing. Three PRs: start without IOReport hardware
      counters, with CPU usage from `host_processor_info`; hide unavailable power, with
      a VM flag in the SoC info and the Prometheus gauges left out; GPU usage from the
      IOAccelerator client time.
      Plan (first PR): [docs/plans/completed/20261012-vm-start.md](plans/completed/20261012-vm-start.md).
      Plan (second PR): [docs/plans/completed/20261012-vm-power.md](plans/completed/20261012-vm-power.md).

- [x] **13. Per-interface network monitoring.** Real-time receive/transmit rates and cumulative
      byte counters without sudo, in the TUI, JSON, Rust library, and Prometheus exporter.
      Keep interfaces separate to avoid double-counting VPN or bridge traffic.
      Plan: [docs/plans/completed/20261013-network-monitor.md](plans/completed/20261013-network-monitor.md).

Later, only on request: automatic grouping of an app's processes (filter terms with totals
cover the known-app case), per-process network (only `nettop` has it without root), history
graphs per process, Activity Monitor's Memory and Statistics tabs (context switches, faults,
Mach messages and system calls, virtual and shared/private memory), Mach port counts and recent
hangs (need a task port or private API).

Cleanup, no hurry: the Mach calls (`mach_host_self`, `mach_task_self`) come from `libc`, which
deprecates them in favor of the `mach2` crate, so they sit under `#[allow(deprecated)]`
(JohnTitor/mach2#34). Declare them with our own `extern "C"`, or move to `mach2` once it has
what we need.

## Personal fork maintenance

- [x] **14. Keep personal fork features while syncing upstream.** A one-command, isolated merge
      with checks and a PR in `Spacgithub/macmon`; sync PRs preserve upstream history, and
      inherited publication jobs remain restricted to the original repository.
      Plan: [docs/plans/completed/20261013-upstream-sync.md](plans/completed/20261013-upstream-sync.md).

- [x] **15. Complete the first upstream sync with Network Monitoring preserved.** Integrate
      upstream VM improvements, coordinate the network layout, and validate the combined fork.
      Plan: [docs/plans/completed/20261014-first-upstream-sync.md](plans/completed/20261014-first-upstream-sync.md).

- [x] **16. Polish the terminal UI while avoiding unchanged redraws.** Add terminal-theme metric
      accents and aligned network values, retain complete narrow power titles, and redraw on
      resize, input or fresh data while preserving Network Monitoring and VM behavior.
      Plan: [docs/plans/completed/20261014-ui-polish.md](plans/completed/20261014-ui-polish.md).

- [x] **17. Refine the terminal dashboard hierarchy.** Separate brand and hardware metadata,
      emphasize values over units, align wide tables and keep the latest sample prominent.
      Continue PR #4 with the existing sampling, controls and terminal-theme compatibility.
      Plan: [docs/plans/completed/20261014-ui-refinement.md](plans/completed/20261014-ui-refinement.md).

- [x] **18. Add the selected deep-space console and bounded fan motion.** Group wide-screen
      telemetry, supply an ANSI-compatible Warp theme and add a saved motion switch with
      visibility/focus gating, real RPM and measured resource cost.
      Plan: [docs/plans/completed/20261014-deep-space-console.md](plans/completed/20261014-deep-space-console.md).

- [x] **19. Add full graphical cooling panels.** Seven curved, shaded blades per real fan,
      independent 60 FPS image updates and the existing manual switch, with terminal capability
      detection, adaptive placement, owned image cleanup and measured resource cost.
      Plan: [docs/plans/completed/20261014-full-fan-panels.md](plans/completed/20261014-full-fan-panels.md).

- [x] **20. Match fan materials to the accepted cooling-card prototype.** Add the rounded
      mechanical housing, recessed screws, cavity and hub shadows, and layered curved-metal
      blades while preserving the independent frame clock, palette and existing controls.
      Plan: [docs/plans/completed/20261014-fan-material-fidelity.md](plans/completed/20261014-fan-material-fidelity.md).

- [x] **21. Keep graphical cooling cards compact.** Reduce fan images and card footprints,
      reclaim graph/process space and retain the existing materials and animation controls.
      Plan: [docs/plans/completed/20261014-compact-cooling.md](plans/completed/20261014-compact-cooling.md).

- [x] **22. Adapt complete cooling fans across terminals.** Continuously size compact cards
      using physical cell dimensions, retain verified native images and add complete ANSI
      text fans with the same rotor state, independent clock and controls.
      Plan: [docs/plans/completed/20261014-terminal-fans.md](plans/completed/20261014-terminal-fans.md).

- [x] **23. Add a local browser dashboard.** Reuse real samples for responsive CPU/GPU,
      memory and Network views, compact material fans and reliable connection/sample state.
      Plan: [docs/plans/completed/20261014-local-dashboard.md](plans/completed/20261014-local-dashboard.md).

- [x] **24. Add dashboard Processes and named temperatures.** Reuse completed process
      snapshots for sortable CPU/GPU/memory rows and show prominent CPU/GPU Celsius readings.
      Plan: [docs/plans/completed/20261014-dashboard-processes.md](plans/completed/20261014-dashboard-processes.md).

- [ ] **25. Control live dashboard sampling.** Adjust the service's real sampling interval
      with local web controls, effective/pending feedback and coordinated polling/freshness.
      Plan: [docs/plans/20261014-dashboard-interval.md](plans/20261014-dashboard-interval.md).
