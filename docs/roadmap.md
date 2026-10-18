# Roadmap

Items labelled **Upstream** retain the original repository's plans. Completed personal-fork
features, including Network Monitoring, memory pressure and disk health, are recorded below.

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
      them earn a place in an 80-column window. Done: CPU TIME (`TIME`, before `CPU%`).
      Plan: [docs/plans/completed/20261014-cpu-time.md](plans/completed/20261014-cpu-time.md).
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
- [x] **8. macmon's own cost, before the release.** Measure what the TUI, `pipe` and `serve` cost
      at 1000 ms and 250 ms: CPU time per minute and power (macmon's own POWER, `powermetrics`
      task energy, Activity Monitor's Energy Impact), with the process list shown and hidden.
      Profile the hot paths (sampling, IORegistry GPU reads, `ps`, rendering) and fix what is
      cheap to fix; report before / after numbers in the PR.
      Plan: [docs/plans/completed/20261014-own-cost.md](plans/completed/20261014-own-cost.md).
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
- [ ] **Upstream 13. Social preview card.** The repository still shows GitHub's template image. A
      1280×640 card from a real TUI screenshot of the current release (HTML rendered to PNG),
      uploaded by hand in the repository settings.
- [ ] **Upstream 14. `serve` as a LaunchDaemon.** For the next minor release. `serve --install` creates a
      LaunchAgent, which starts only after login; an always-on Mac scraped by Prometheus needs it
      at boot with nobody logged in (asked on Reddit). An install option for a LaunchDaemon in
      `/Library/LaunchDaemons` (needs sudo). Tested by hand in the VM: a daemon running `macmon
      serve` starts at boot without a login, as root and with `UserName`; with `UserName` the
      `StandardErrorPath` file must be writable by that user.
- [ ] **Upstream 15. Disk and network metrics.** What node exporter has and macmon doesn't (asked on
      Reddit): disk read / write and network in / out, system-wide, in the TUI, JSON and
      Prometheus. Decision point before the plan: which of them, and where they fit in the TUI.
- [ ] **Upstream 16. App totals with their helpers.** Asked on Reddit: a filter term totals only the
      processes whose name matches, so Safari's `com.apple.WebKit.WebContent` tabs are left out.
      Group an app's helper processes under it, expandable to show which ones are counted. Until
      then, the total row could say it covers matching processes only.
- [ ] **Upstream 17. Memory pressure.** Asked on Reddit: RAM use on a Mac is nearly always 70%+ and says
      little; memory pressure, as Activity Monitor shows it, tells whether the Mac is short of
      memory. Read it without root (sysctl `kern.memorystatus_vm_pressure_level` and
      `kern.memorystatus_level`, to verify), color the RAM box by it and add it to JSON and
      Prometheus. Decision point before the plan: next to RAM use or instead of it in the title.
- [ ] **Upstream 18. Where a process works.** With agents building and testing in several projects at
      once, the busiest process is often just `rustc`, `cargo` or `node`, and the name doesn't
      say which project it belongs to. Show the working directory in the list (a column, or the
      NAME cell for such tools, `~` for home, cut from the left), so the project is visible while
      scrolling, without opening `i`. Other users' processes show no directory (libproc needs
      root). Decision point before the plan: a column, a second line, or a toggle.
- [ ] **Upstream 19. Who is busy lately.** CPU% is one sample and jumps; TIME covers the whole life of a
      process. To find what loads the Mac right now (a runaway process, an agent's test run, a
      build), show CPU used over a recent window, e.g. the last 15 s or 1 min, from the CPU time
      already sampled, and sort by it. Decision point before the plan: a new column or a switch
      of TIME between "total" and "recent", and the window length.

Order for 0.11: 10, then 5 (decide which columns fit 80 cells; 6 only if too many), the second
PR of 7, the third PR of 12. Items 11 and 4 need their decision first. Items 10, 12 and 13 are
small and self-contained, good for an outside contributor.

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

- [x] **25. Control live dashboard sampling.** Adjust the service's real sampling interval
      with local web controls, effective/pending feedback and coordinated polling/freshness.
      Plan: [docs/plans/completed/20261014-dashboard-interval.md](plans/completed/20261014-dashboard-interval.md).

- [x] **26. Add desktop control for background web monitoring.** Start/open and stop a
      precisely owned launchd job from a small macOS app, preserving login preferences and
      separate terminal monitors.
      Plan: [docs/plans/completed/20261014-dashboard-control.md](plans/completed/20261014-dashboard-control.md).

- [x] **27. Show detailed browser power readings.** Distinguish the CPU+GPU+ANE subtotal
      from the system estimate, show component watts and retain a scaled 60-sample history.
      Plan: [docs/plans/completed/20261016-dashboard-power-details.md](plans/completed/20261016-dashboard-power-details.md).

- [x] **28. Control the browser dashboard from the terminal.** Add native start/open,
      start-only, stop and status commands with discoverable help, reusing the managed
      service without changing login preferences or separate terminal monitors.
      Plan: [docs/plans/completed/20261016-web-cli.md](plans/completed/20261016-web-cli.md).
      Existing executable aliases: [compatibility plan](plans/completed/20261016-web-cli-alias-compatibility.md).

- [x] **29. Retain detailed temperature readings.** Publish same-sample named CPU/GPU
      sensor readings without changing existing metric structs or legacy outputs.
      Plan: [docs/plans/completed/20261017-temperature-details.md](plans/completed/20261017-temperature-details.md).

- [x] **30. Add browser overview/detail controls.** Per-panel and global expansion,
      temperature history and named sensors, with concise live summaries.
      Plan: [docs/plans/completed/20261017-web-panel-details.md](plans/completed/20261017-web-panel-details.md).

- [x] **31. Add terminal overview/detail controls.** Keyboard-accessible per-panel and
      global expansion, full lists and temperature details with compatible preferences.
      Plan: [docs/plans/completed/20261017-terminal-panel-details.md](plans/completed/20261017-terminal-panel-details.md).

- [x] **32. Show memory pressure and swap activity.** Read the operating system's pressure
      level and independent swap-in/out page-equivalent rates, with compatible snapshots and
      concise terminal/browser summaries and details.
      Plan: [docs/plans/completed/20261017-memory-activity.md](plans/completed/20261017-memory-activity.md).

- [x] **33. Show the operating system's thermal state.** Read the public ProcessInfo state
      and display it alongside existing temperatures and fan readings, without inferring
      state from individual sensor thresholds or changing legacy outputs.
      Plan: [docs/plans/completed/20261017-system-thermal-state.md](plans/completed/20261017-system-thermal-state.md).

- [x] **34. Show physical disk activity and supported SSD health.** Add per-device read/write
      rates and IOPS, native NVMe wear/health data with freshness and honest fallbacks, and
      compact Terminal/browser summaries with independently expandable details.
      Plan: [docs/plans/completed/20261017-disk-monitoring.md](plans/completed/20261017-disk-monitoring.md).

- [x] **35. Make compact summaries and expanded details useful.** Add real CPU tier/core
      breakdowns and GPU details, a named Network observation overview, and responsive
      Terminal/browser layouts that preserve all disk and existing monitoring features.
      Plan: [docs/plans/completed/20261017-overview-details-layout.md](plans/completed/20261017-overview-details-layout.md).

- [x] **36. Group applications and their helper processes.** Share confirmed application
      membership and honest totals across Terminal and Web, with independent expansion,
      a per-process view and safe actions on explicitly selected process members.
      Plan: [docs/plans/completed/20261018-process-grouping.md](plans/completed/20261018-process-grouping.md).

- [ ] **37. Reduce combined Terminal and Web resource use.** Share a rootless sampling
      source, collect process details at a lower rate on demand, and provide animation FPS
      controls with measured CPU/memory comparisons and compatible outputs/preferences.
      Plan: [docs/plans/20261018-resource-efficiency.md](plans/20261018-resource-efficiency.md).
