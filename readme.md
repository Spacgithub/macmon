# `macmon` – Mac Monitor

<div align="center">

[<img src="https://badges.ws/github/assets-dl/vladkens/macmon" />](https://github.com/vladkens/macmon/releases)
[<img src="https://badges.ws/github/release/vladkens/macmon" />](https://github.com/vladkens/macmon/releases)
[<img src="https://badges.ws/github/license/vladkens/macmon" />](https://github.com/vladkens/macmon/blob/main/LICENSE)
[<img src="https://badges.ws/badge/-/buy%20me%20a%20coffee/ff813f?icon=buymeacoffee&label" alt="donate" />](https://buymeacoffee.com/vladkens)

</div>

`macmon` is a sudoless performance monitor for Apple Silicon Macs. It reads real-time CPU / GPU / ANE power usage, temperatures, and memory stats through a private macOS API — the same data `powermetrics` exposes — without requiring root access.

<div align="center">
  <img src="https://github.com/vladkens/macmon/blob/assets/macmon-v0.9.png?raw=true" alt="preview" />
</div>

## 🌟 Features

- 🚫 Runs without sudo
- ⚡ Real-time CPU / GPU / ANE power usage
- 📊 CPU frequency-scaled and active ratios per cluster
- 💾 RAM / Swap usage, system memory pressure and swap-in/out rates
- 🌐 Per-interface network receive/transmit rates and cumulative traffic
- 📈 Historical charts with average and max values
- 🌡️ Average CPU / GPU temperature and OS thermal state
- 📋 Expandable application groups and individual processes with CPU, memory, power and GPU usage
- 🎨 Follows your terminal's color scheme
- 🪟 Can be displayed in a small window
- 🦀 Written in Rust

## 📥 Installation

Install [`macmon`](https://formulae.brew.sh/formula/macmon) using [brew](https://brew.sh/):

```sh
brew install macmon
```

<details>
<summary>Other installation methods</summary>

Install using [MacPorts](https://ports.macports.org/port/macmon/):

```sh
sudo port install macmon
```

Install using [Cargo](https://crates.io/crates/macmon):

```sh
cargo install macmon
```

Install using [Nix](https://search.nixos.org/packages?show=macmon&from=0&size=50&type=packages&query=macmon):

```sh
nix-env -i macmon
```

</details>

## 🚀 Usage

```sh
Usage: macmon [OPTIONS] [COMMAND]

Commands:
  pipe    Output metrics in JSON format
  serve   Serve metrics over HTTP
  debug   Print debug information
  stress  Generate load for testing metrics
  help    Print this message or the help of the given subcommand(s)

Options:
  -i, --interval <INTERVAL>  Update interval in milliseconds [default: 1000]
  -h, --help                 Print help
  -V, --version              Print version
```

### Interactive mode

Run `macmon` without a subcommand to open seven compact panels: Compute, Memory, Power,
Network, Cooling, Processes and Disk. At 80×24 their live summaries keep one named automatic
Network observation, five sorted application/process entries, current CPU/GPU temperatures, fan RPM and the
busiest physical disk visible. Disk keeps read/write rates separate; IOPS appears when the
whole field fits.

Terminal windows and HTTP backends share one rootless system and process sampling source for
this user. A standalone terminal needs no login service: the temporary collector exits after
its final consumer closes. An existing web backend keeps collecting until it is stopped,
including while browser tabs are closed. `pipe` keeps its independent sampling behavior.
Changing the sampling interval in either interface updates the shared source and other
connected views; only the initiating terminal saves its preference. A reconnect waits for a
fresh source instead of opening a second sampler, and source restarts reset rate baselines.
After updating macmon, restart existing monitors and the web backend together.

Use `1`–`7` to toggle a panel, `Tab` / `Shift-Tab` to select one, and `Enter` to toggle the
selected panel. `d` expands all panels or returns them to summaries. `f` opens the selected
panel in a full-screen detail view; `f` / `Esc` returns. Use arrows, Page Up/Down or Home/End
to scroll complete core, interface and sensor lists. Process arrows retain their selection behavior
and open the full list when needed. Filtering, sorting and selected processes survive detail
changes and resizing; `p` separately hides the process panel and releases this terminal's
process demand. Other connected views can keep shared process collection active.

Expanded Compute adds the original overall CPU activity, reported/installed core counts,
core-type summaries and every reported die/core. GPU details add reported activity, supported
frequency range, core count, power and history. `r` retains the chosen Active/Weighted mode;
missing weighted measurements are labelled unavailable with reported activity kept separate.
Core/GPU MHz describe active-time means; CPU tier aggregates include a minimum-frequency
floor. Tick fallbacks, incomplete coverage and VM readings do not establish weighted frequency
measurements, and a zero GPU report does not prove idle. Use `1` then `f` for the full analysis.

Network summaries show the selected Auto interface, separate receive/transmit rates and
observed/traffic-active/rates-pending counts. At narrow widths these counts are abbreviated
N/A/P. Traffic and pending counts can overlap. Expanded Network adds every interface's
rates and cumulative bytes; narrow cards wrap complete fields and scroll through the list.

Expanded Cooling shows each group's current sensor count/range, the current hotspot's raw
name, a 60-sample mean-temperature trend with gaps for missing readings, historical mean
statistics and every sensor's original name, source and Celsius. Use `f` to give long lists
more room. Folding Cooling pauses its fan drawing while RPM and temperature sampling continue.
The optional `panel_details` setting keeps the original six expansion choices; `disk_details`
saves Disk independently. Old settings remain compatible, and the temporary `f` view is not
saved. Press `?` for help.

Select Disk with `7` and press `f` for every physical device's IOPS, driver read/write counters
and supported NVMe health fields. Driver counters are separate from lifetime host read/write
bytes and exact NVMe data units (512,000 bytes each); neither measures physical NAND writes.
Endurance used may exceed 100% (`>254%` for NVMe's 255 sentinel), not a prediction of remaining
years. Unsupported and unavailable health are distinct; reports older than 120 seconds say
STALE. The summary says NVMe OK or NVMe warning; details retain the original hexadecimal
critical-warning bit field.

When all panels are expanded, the advanced chart layout remains available. At least 100
columns and 30 rows show Compute on the left, Memory and a Power/Temperature ledger on the
right, and full-width Network and Processes below. Smaller windows and VMs use the adaptive
metric grid. This overview keeps three Network rows when room permits; select Compute,
Network or Cooling and press `f` for full core, interface or named-sensor details. Disk has its
own full-width strip. When a small
advanced window needs space for complete fans and Disk, Processes shows a concise entry;
`6` then `f` still opens the full process list without changing the sampling preference.

CPU, GPU and RAM charts use terminal-theme accents; high-load history remains red. Primary
values are bold, while secondary details are dimmed. Network receive/transmit rates and totals
are aligned, with arrows marking their direction. Narrow power boxes reduce precision while
keeping each displayed number and its unit complete.

The hardware header separates the brand, chip and specifications, with complete components
omitted when space is limited. Units and process metadata are quieter than primary values;
the latest chart sample and hot-load history stay prominent. Wide network tables align their
totals to the panel edge while preserving the compact layout in narrow windows.

Compatible Warp, Kitty and WezTerm sessions can show a complete, shaded seven-blade fan for
each reported fan, up to 120 pixels across, with a recessed
mechanical housing, corner screws and curved metal highlights. The housing and hub stay fixed
as the blades turn. Cards stay compact in large windows and preserve room for the process list.
Fan diameter adapts to available space and physical cell dimensions, with RPM on the card's
bottom border. Images use the terminal's queried colors and require a successful capability query for the
actual image format and transport. Warp receives
images inline; Kitty and WezTerm use private local files. Apple Terminal and other text terminals
show the same complete fan geometry with ANSI-colored Braille characters. When a complete fan
cannot fit, the compact RPM indicator preserves room for metrics, Network and processes.
Window and font changes refresh pixel sizing through the terminal's window-size reports.
If those reports omit pixels after a resize, macmon safely switches to text with an estimated
1:2 cell aspect ratio; restart macmon to query native image dimensions again in such sessions.

Each rotor follows its own reported RPM; zero RPM stays stationary and unavailable fans stay
absent. Press `a` to toggle animation (enabled by default, saved as `animate_fans`). Press
`Shift+A` to cycle the target between 15, 30 and 60 FPS; the default is 30 FPS, saved as
`fan_fps`. This changes actual animation wakeups independently of metric/process sampling;
compact indicators still change at most four times per second. Animation pauses on focus
loss, under help and when hidden, preserving your manual choice. It never controls hardware
fans or changes the saved sampling interval (1000 ms by default).

The screen redraws when data, input, the window size or a visible fan frame changes. Other input
heartbeats skip drawing, and interfaces are ranked when samples arrive. Existing configuration
fields, including `Sparkline` and `Gauge`, remain compatible.

For the optional deep blue Warp palette, copy [macmon-deep-space.yaml](themes/macmon-deep-space.yaml)
to `~/.warp/themes/`, then select **Macmon Deep Space** in Warp's Theme Picker
(`Ctrl+Cmd+T`). See [Warp's custom theme guide](https://docs.warp.dev/terminal/appearance/custom-themes/).
The program uses ANSI colors and also follows other terminal palettes; it does not change your
terminal preferences automatically.

The process panel starts with application totals. Press `g` to switch between applications
and individual processes; this choice is saved. In the full list (`6` then `f`), select an
application and press Right to expand its members or Left to collapse it. Left on a member
returns to its application; click only the arrow to toggle with the mouse. Each application
keeps its expansion during refresh, sorting and filtering for the current run. Compact views
show five root entries, without letting expanded children displace other applications.
`×N` is a member count, never a PID. Information, Finder, copying and termination act only
on an explicitly selected real process; application summaries have no such action.

Press `/` to filter. Comma-separated terms match application/process names or PIDs without
case sensitivity: `safari, cargo` matches either, while `chrome, !helper` excludes matching
helpers. Application totals always include the full membership; the matching/total count
describes the visible children. In individual mode, the original filter-term totals remain
available. `Enter` keeps the filter and `Esc` clears it after leaving a full-panel view.
Grouped CPU 100% is one core, memory sums reported process memory, and GPU totals sum
process shares. `≥` marks a partial total; `-` means unavailable. Large values retain their
units using compact notation. Unconfirmed application ownership remains an individual row.

Complete process snapshots refresh every 5 seconds in a summary, or every 1 second while a
detailed list is requested. A slower system interval takes precedence. The highest demand
among connected views wins; with no demand, collection pauses and retained data keeps its
original age. Unfocused terminals use the summary cadence. A process Info overlay always
requests details, but reads only that selected PID's files and sockets on a separate worker,
every 5 seconds (or the slower system interval). Press `r` in Info to refresh immediately;
while entering its filter, `r` remains text. Closing Info discards late results.

### JSON output

You can use the `pipe` subcommand to output metrics in JSON format, which makes it suitable for piping into other tools or scripts. For example:

```sh
macmon pipe | jq
```

This command runs `macmon` in "pipe" mode and sends the output to `jq` for pretty-printing.

You can also specify the number of samples to collect using the `-s` or `--samples` parameter (default: `0`, which runs indefinitely), and set the update interval in milliseconds using the `-i` or `--interval` parameter (default: `1000` ms). For example:

```sh
macmon pipe -s 10 -i 500 | jq
```

This will collect 10 samples with an update interval of 500 milliseconds.

### HTTP server

You can use the `serve` subcommand to expose metrics over HTTP. This is useful for integrating with monitoring systems like [Prometheus](https://prometheus.io/) and [Grafana](https://grafana.com/).

```sh
macmon serve                   # default port 9090, interval 1000ms
macmon serve --host 127.0.0.1  # listen on localhost only
macmon serve -p 8080           # custom port
macmon serve -i 500            # sampling interval 500ms
macmon serve &                 # run in background
```

Available endpoints:

| Endpoint       | Format     | Description                                                                                       |
| -------------- | ---------- | ------------------------------------------------------------------------------------------------- |
| `GET /json`    | JSON       | Current metrics snapshot (same format as `pipe --soc-info`)                                       |
| `GET /metrics` | Prometheus | Metrics in [Prometheus text format](https://prometheus.io/docs/instrumenting/exposition_formats/) |
| `GET /dashboard/` | HTML | Local browser dashboard with CPU/GPU activity and temperature, memory, Disk, Network, cooling and Processes |
| `GET /api/sample` | JSON | Successful-sample sequence, timestamp, age, interval, service cadence, metrics and named temperature readings; local-only process snapshot |
| `POST /api/interval` | JSON | Adjust real service sampling with `{"adjustment":"increase"}` or `"decrease"`; local same-origin requests only |

#### Local dashboard

Run `macmon serve --host 127.0.0.1`, then open
[http://127.0.0.1:9090/dashboard/](http://127.0.0.1:9090/dashboard/).
The dashboard is bundled into the binary and uses no external assets or frontend build step.
It follows the service sampling interval and retains 60 distinct samples. CPU/GPU activity uses
active residency ratios. Each panel starts with a compact summary; use its **Details** / **Less**
button or **Expand all** / **Collapse all** to choose how much to show. These choices are saved
locally in the browser and remain usable when browser storage is blocked. Balanced summary
cards keep CPU/GPU activity, their temperatures and RAM/OS pressure visible. Each opened
CPU, GPU or memory detail section gets the full width below these cards. Memory details add
Swap-in/out rates and history. Network and Cooling also use the full width when either is
expanded. Disk summaries retain the three busiest physical devices; Processes retains five
application or process entries in the selected sort order. Folding a panel keeps live sampling
and summaries running.

CPU details retain the reported overall activity, core-type breakdown, reported/installed
core counts and each reported die/core's activity. GPU details show reported activity, core
count, active-time mean frequency and supported frequency range. Both add reported power
and activity versus frequency-weighted history. Frequency-weighted values describe residency,
not throughput. Incomplete CPU coverage, tick fallbacks and virtual machines keep valid
activity without claiming verified frequency data. A GPU report of zero activity with zero
frequency does not establish that the GPU is idle; its frequency-weighted value stays unavailable.

The Network overview names one automatically observed interface, preferring reported rates
and then the busiest known receive/transmit traffic with interface-name tie breaks. It shows
receive/transmit rates separately, plus observed, traffic-active and rates-pending counts;
the last two may overlap when one direction is pending. Collecting, unavailable, empty,
pending rates and measured idle are distinct. These are monitoring states, not Internet
connectivity checks. Expand for all interfaces and cumulative counters; VPN and bridge
traffic is never summed into a whole-system total.

The Power panel separates the system estimate from the CPU + GPU + ANE subtotal and plots
both on a shared watt scale over the latest 60 distinct samples. CPU, GPU, ANE, DRAM and GPU
SRAM readings retain small positive values in milliwatts. The system estimate uses the larger
of the system sensor reading and the CPU + GPU + ANE subtotal; it is not wall-socket power.
Memory readings are separate and should not be added to either total. Missing, invalid and
virtual-machine power stays unavailable; system zero is unavailable. Component zero remains
a reported value because unsupported counters can also report zero.

The Disk card shows separate read/write throughput and IOPS for each physical device. Expand
for every disk, driver cumulative bytes and operations, optional device information, and
reported NVMe health. The selected disk has its own 60-sample read/write history; reconnecting
with a new registry identity starts a new history. Health age includes the cached reading,
sample age and time in the browser; data over 120 seconds old is marked stale, and offline
or stale samples are retained rather than presented as current health. Unsupported health
does not mean healthy. Endurance used can exceed 100%; raw 255 means more than 254%, and no
remaining life or lifetime guarantee is inferred. NVMe lifetime host read/write totals use
512,000 bytes per data unit and stay separate from driver totals and physical NAND writes.
The browser validates the lossless 128-bit decimal counters with bounded `BigInt` conversion;
overflowing byte totals and driver integers beyond JavaScript's exact range stay unavailable.
Older backends without `disks` remain usable and show disk telemetry as unavailable.

Named CPU and GPU temperature readings show Celsius or `Unavailable`. Applications & processes
defaults to application totals, with a button on each application to expand its real member
processes and PIDs. Use View to switch to individual Processes; the browser remembers this
choice. Each application's expansion lasts for the current page and survives live refresh,
sorting and filtering. Details shows every root entry, while the compact view keeps the top
five; panel/global expansion does not automatically open every application's members.
Filter searches application names, process names and PIDs. A member-only match shows the
matching/total count; application totals still include all members. Unknown ownership stays
as an individual process. Older backends without grouping data retain the individual list.
CPU, GPU and memory are sortable by usage; CPU 100% means one busy core. New processes and
unavailable rates show `Unavailable`; partial totals show `≥` and known-member coverage.
Visible browser summaries request the 5-second process cadence; expanded lists request
1 second while focused, and hidden tabs release their demand. Only loopback clients with a matching loopback Host
receive process names and PIDs; other requests get `processes: null`. Snapshot age is reported
as `processes_age_ms`, and all processes remain accessible in the scrollable table.
System and process collection run independently. Their separate timestamps and ages retain
the actual completion time; a slower process read does not delay publication of system metrics.
`process_collection` reports the collector session, process sequence, state, aggregate demand
and actual interval. Browser demand uses bounded 15-second leases through a same-origin
loopback `POST /api/process-demand`; leases expire even when tabs stop making requests.
Older clients without a lease keep their existing full-snapshot behavior while polling
`/api/sample`. Remote system metrics and older backends remain supported.

The same local snapshot includes `process_groups`: application summaries and unassigned
individual processes, with opaque group IDs and `{pid, instance}` member identities. Confirmed
application packages include their embedded helpers; standalone XPC services join only when
verified system grouping metadata matches one observed application for the same user. Ambiguous
or unsupported ownership stays independent. Full application totals include every member;
`cpu_known` and `gpu_known`
distinguish partial totals from measured zero. Memory is the sum of reported process memory,
not deduplicated physical memory; GPU totals sum process shares rather than device utilization.
The existing five-field `processes` rows and their age stay compatible. Both arrays are `null`
outside the loopback privacy gate; executable paths and command lines are never included.

`/api/sample` also includes a top-level `temperatures` array from the same successful hardware
sample, for example `{"name":"Tp01","group":"cpu","source":"smc","celsius":45.0}`.
Each entry retains its original sensor name, the existing CPU/GPU group and SMC/HID source.
SMC grouping is heuristic: these names do not establish physical core, battery or storage
locations. Only finite readings above 0 and at most 150 °C are retained; missing detail is an
empty array. Library callers can use `Sampler::temperature_details()` after `get_metrics()`;
the accessor performs no hardware reads and is empty before sampling or after a failed sample.
Existing `Metrics`/`TempMetrics` structs, `pipe`, `/json` and Prometheus formats are unchanged.

Memory summaries also show the operating system's **Normal / Warning / Critical** memory
pressure and independent **Swap in / Swap out** rates. Pressure is a system assessment,
separate from RAM percentage. Expand Memory for used/total swap and the complete rates.
These rates use cumulative VM swap-page counters, the actual page size and monotonic time:
they are page-equivalent bytes per second, not SSD physical I/O or wear. A first sample,
missing source, counter reset or invalid interval shows `Unavailable`; a valid idle interval
shows `0 B/s`. Unsupported pressure levels stay unavailable. In the terminal’s expanded
overview, `I` and `O` mean swap-in and swap-out; press `f` for the full labels. See Apple’s
[memory-pressure guide](https://support.apple.com/en-asia/guide/activity-monitor/actmntr1004/mac)
for how macOS uses memory pressure alongside usage and swap.

Library callers can read `Sampler::memory_activity()` after a successful `get_metrics()`;
`/api/sample` exposes the same snapshot as a top-level `memory_activity` object with
`pressure` (`normal`, `warning`, `critical` or `null`), `swap_in_bytes_per_sec` and
`swap_out_bytes_per_sec` (numbers or `null`). The accessor performs no new hardware reads
and clears on acquisition failure. Existing `Metrics` / `MemMetrics`, `pipe`, `/json`,
Prometheus and saved configuration formats remain compatible.

Expand **Temperature & cooling** for each group's current sensor count, minimum, maximum
and hottest raw sensor name, the full source-labelled sensor list, and a 60-sample CPU/GPU
mean-temperature history. History peaks describe recorded group means; current sensor maxima
describe individual readings. Missing values leave gaps. The compact view keeps means and
fan RPM visible, while folding Cooling pauses its fan animation.

Cooling also displays the macOS-reported **Nominal / Fair / Serious / Critical** thermal
state in both interfaces. This is the public
[ProcessInfo thermal state](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.enum),
independent of the sensor temperatures; it is not a Celsius threshold or proof of CPU/GPU
throttling. Apple can also report Nominal when the platform's thermal state is unknown or
unsupported. Virtual machines, missing APIs and unrecognized values show `Unavailable`.
Expand Cooling for the state explanation alongside the existing sensors and fan readings.

`Sampler::thermal_state()` returns the cached `Option<ThermalState>` from the latest successful
acquisition, or `None` before sampling or after a failed acquisition. `/api/sample` adds a
top-level `thermal_state` string (`nominal`, `fair`, `serious`, `critical`) or `null`. Existing
metric structs, legacy outputs and preferences remain unchanged.

The −/+ controls change the service's real sampling window in 250 ms steps, from 250 to
10000 ms, using the terminal's adjustment rules. The displayed interval becomes effective
when the next acquisition starts; a requested change remains pending while the current
acquisition finishes. Further adjustments are allowed while pending. These runtime changes
do not write terminal settings or change the command's startup interval. Existing CLI
intervals down to 100 ms remain supported. Fan motion stays independent of sampling.

Control requests require a loopback client, a loopback `Host` with the service's actual port,
an exact same-origin `Origin`, and JSON content type. Remote dashboards remain read-only.
`/api/sample` reports the completed window in `interval_ms` and current service state in
`cadence: {requested_ms, effective_ms, pending}`. Pending settings are checked at least once
per second; sample age uses completed and effective windows, so an unstarted request cannot
hide stale readings.

Compact mechanical fans show reported names and RPM. Zero RPM remains stationary; missing
readings and VM-only placeholders stay unavailable. Cooling's Target FPS selector offers
15, 30 and 60 FPS, initially 30, saved separately in browser storage. The Fan motion switch
and reduced-motion preference remain independent of this rate and of sampling. Hidden pages pause polling
and animation. WAITING, LIVE, STALE and OFFLINE describe connection/sample state while keeping
the last available values visible. `/api/sample` adds freshness metadata; `/json`, `/metrics`,
the root JSON response and existing server defaults retain their formats.

See [resource efficiency validation](docs/resource-efficiency.md) for sampling, animation
and matched CPU/memory measurement details.

#### Terminal web control

```sh
macmon web          # start background monitoring and open the dashboard
macmon web start    # start in the background without opening a browser
macmon web status   # show service state and dashboard address
macmon web stop     # stop the background job, sampling and listener

macmon -h           # list all commands
macmon web -h       # explain web controls and default behavior
macmon web stop -h  # help for an individual action (--help also works)
```

Repeated starts reuse the running service. Closing a browser tab keeps monitoring active;
use `macmon web stop` to end it. An existing tab stays open and reports OFFLINE after stopping.
These commands use the same controller as the desktop entry below, bundled into the binary
so this checkout is not required. Without a compatible login agent, the service listens only
on `http://127.0.0.1:19090/dashboard/` for the current login session. Existing login startup
preferences and separate terminal monitors are preserved.
An existing executable alias is reused when it points to the same file; its configured
path and service plist are preserved.
New services keep the executable path reported by macOS, preserving compatibility with
older desktop entries when starting through the same alias.

#### Desktop control

Create a small macOS app once, using an absolute path to the installed executable:

```sh
sh scripts/dashboard-control.sh install "$HOME/.local/bin/macmon" "$HOME/Desktop/macmon Web Control.app"
```

Double-click **macmon Web Control**, then choose **启动并打开** to start monitoring and open
the dashboard, or **停止** to stop the background job. Closing the browser alone keeps
monitoring active. The app contains its own control script and does not need this checkout.
Without an existing login agent it uses a session-only, loopback listener at
`http://127.0.0.1:19090/dashboard/`; its plist stays under
`~/Library/Application Support/macmon/dashboard-control/`, outside `LaunchAgents`.
A compatible `com.macmon` login agent keeps its executable, host and port; an optional
`Program` must name that same executable. Only exact `serve --host … --port …` arguments
with `127.0.0.1` or `0.0.0.0` are accepted. Stopping
unloads the current job so KeepAlive cannot restart it, while preserving the login plist
and startup preferences. Port conflicts and foreign jobs are rejected; a separate TUI is
unaffected. The script also supports headless `start`, `status` and `stop` with the same
executable argument. `open` starts the job and opens the browser only after readiness.

#### launchd service

To start `macmon serve` automatically on login and keep it running:

```sh
macmon serve --install                   # install and start (default port 9090)
macmon serve --port 8080 --install       # with custom port
macmon serve --host 127.0.0.1 --install  # listen on localhost only
macmon serve --uninstall                 # stop and remove
```

This creates a launchd agent at `~/Library/LaunchAgents/com.macmon.plist` that auto-starts on login and restarts on crash.

#### Prometheus and Grafana

The `/metrics` endpoint exposes metrics in Prometheus format. See [`examples/grafana`](examples/grafana) for a local demo stack with Prometheus and Grafana.

<details>
<summary>Prometheus output example</summary>

```
# HELP macmon_cpu_temp_celsius Average CPU temperature in Celsius
# TYPE macmon_cpu_temp_celsius gauge
macmon_cpu_temp_celsius{chip="Apple M3 Pro"} 47.3

# HELP macmon_cpu_power_watts CPU power consumption in Watts
# TYPE macmon_cpu_power_watts gauge
macmon_cpu_power_watts{chip="Apple M3 Pro"} 8.42

# HELP macmon_fan_speed_rpm Fan speed in revolutions per minute
# TYPE macmon_fan_speed_rpm gauge
macmon_fan_speed_rpm{chip="Apple M3 Pro",fan="fan0"} 1234

# HELP macmon_cpu_scaled_ratio Combined frequency-scaled CPU ratio (0–1), weighted by core count
# TYPE macmon_cpu_scaled_ratio gauge
macmon_cpu_scaled_ratio{chip="Apple M3 Pro"} 0.037

# HELP macmon_cpu_active_ratio Combined CPU active residency ratio (not frequency-scaled, 0–1), weighted by core count
# TYPE macmon_cpu_active_ratio gauge
macmon_cpu_active_ratio{chip="Apple M3 Pro"} 0.092

# HELP macmon_cpu_tier_freq_mhz CPU tier frequency in MHz
# TYPE macmon_cpu_tier_freq_mhz gauge
macmon_cpu_tier_freq_mhz{chip="Apple M3 Pro",tier="E"} 1100
macmon_cpu_tier_freq_mhz{chip="Apple M3 Pro",tier="P"} 1800

# HELP macmon_cpu_tier_scaled_ratio CPU tier frequency-scaled ratio (0–1)
# TYPE macmon_cpu_tier_scaled_ratio gauge
macmon_cpu_tier_scaled_ratio{chip="Apple M3 Pro",tier="E"} 0.083
macmon_cpu_tier_scaled_ratio{chip="Apple M3 Pro",tier="P"} 0.015

# HELP macmon_cpu_tier_active_ratio CPU tier active residency ratio (not frequency-scaled, 0–1)
# TYPE macmon_cpu_tier_active_ratio gauge
macmon_cpu_tier_active_ratio{chip="Apple M3 Pro",tier="E"} 0.18
macmon_cpu_tier_active_ratio{chip="Apple M3 Pro",tier="P"} 0.04
```

`macmon_ecpu_*` and `macmon_pcpu_*` are still exported for the lowest and the highest tier. In a macOS VM the power and frequency gauges are left out.

</details>

### Stress testing

Use `macmon stress` to generate load while checking metric behavior:

```sh
macmon stress
macmon stress pulse --duration 30
macmon stress cpu --duration 30
macmon stress cpu --workers 8 --duration 30
macmon stress gpu --duration 30
macmon stress all --duration 30
```

The default `pulse` mode generates a predictable cyclic CPU load with a fixed 50% duty cycle on half of the logical CPUs. The `cpu` and `gpu` modes continuously load only the selected processor, while `all` loads both. The `cpu` and `all` modes use all logical CPUs unless `--workers` is specified; `--workers` has no effect in `gpu` mode.

## 📊 Metrics

### Power sources and macOS 27

macOS 27 broke the CPU/ANE power source that macmon previously used without root. The IOReport `Energy Model` counters can remain visible but stop updating: refreshing them now requires the private `com.apple.private.pmgr.nrg.reporting` entitlement. Apple's signed `powermetrics` has this entitlement and can keep using the named channels. Running macmon with `sudo` does not grant the same access.

The working rootless workaround we found is to read hidden energy reports from the AppleCLPC driver through IOReport. These reports use opaque IDs instead of discoverable CPU/GPU/ANE channel names, so macmon has to identify the counters and verify their units. This leaves macmon maintaining undocumented mappings for data that Apple's own tool can still read directly.

Both the CLI and the Rust library use known CLPC counters automatically, and `Sampler::new()` retains legacy fallbacks for older systems. New chips or driver versions may need additional mappings. On an unsupported configuration, zero CPU/ANE readings can mean unavailable counters rather than zero consumption. See [docs/clpc-discovery.md](docs/clpc-discovery.md) for details.

### Network monitoring

The TUI shows receive and transmit rates and cumulative byte counters for each network
interface that is up, excluding loopback. The compact panel automatically observes one named
interface, preferring available rates then known traffic, with stable name tie breaks. Its
global state distinguishes collecting, unavailable, empty, rates pending, observed traffic
and measured zero. This describes sampling, not Internet connectivity. Expand for all interfaces;
`pipe`, HTTP `/json`, and Prometheus export every sampled interface. Rates use the measured
time between snapshots, including time between calls to the Rust sampler.

```sh
macmon pipe -s 5 | jq '.network'
```

`network` is an array of interface metrics, or `null` if the network source is unavailable.
Rates are bytes per second; a rate is `null` until an interface has two usable samples, or
when its counter decreases. Cumulative bytes come from the kernel and can reset when an
interface restarts. The TUI uses binary byte units (KiB, MiB, GiB). VPN and bridge interfaces
are reported separately: adding their traffic to a physical interface can count the same
data twice.

The public macOS interface MIB supplies 64-bit counters without sudo. Prometheus exposes
`macmon_network_receive_bytes_total` and `macmon_network_transmit_bytes_total` counters, plus
`macmon_network_receive_bytes_per_second` and `macmon_network_transmit_bytes_per_second`
gauges, labeled by `interface`. Unavailable rates have no gauge series for that sample.

### Disk monitoring API

`Sampler::disks()` and the top-level `disks` field in HTTP `/api/sample` expose physical
whole-device I/O. `null` means unavailable; `[]` means a successful scan found no eligible
devices. APFS containers and volumes are excluded. Each disk has its BSD `name`, optional
model, location and capacity, and an `id` identifying the driver and media connection generation.
Rates are bytes/s and operations/s over measured intervals. First samples, missing counters,
counter resets, failed acquisitions and reconnected media need a fresh baseline; idle rates are
real zeroes. Cumulative counters start when the storage driver is instantiated, not at manufacture
([Apple counter definitions](https://github.com/apple-oss-distributions/IOStorageFamily/blob/main/IOBlockStorageDriver.h#L56-L82)).

NVMe health runs in a separate worker, at most once per minute, including failed attempts.
`health.state` is `available`, `unsupported` or `unavailable`; `health.age_ms` records its age
when the containing sample completed. Clients add the sample's current `age_ms` and treat health
older than 120 seconds as stale. A stalled hardware health call does not block normal sampling;
its worker is not replaced while pending. Unknown devices and VM host health stay unavailable.

`health.nvme` includes the critical-warning byte, manufacturer-estimated percentage **used**,
available spare, temperature and lifetime counters. An estimate can exceed 100%; it does not
establish remaining years. All 128-bit counters are decimal strings, preserving precision in JSON.
One reported read/write data unit is 512,000 bytes of host data, not physical NAND writes or wear.
Units and wear estimates follow the [NVMe SMART/Health log specification](https://nvmexpress.org/wp-content/uploads/NVM-Express-Base-Specification-Revision-2.1-2024.08.05-Ratified.pdf).
No self-tests, SMART enable commands, serial-number queries or root permissions are used.
External NVMe support depends on the macOS driver exposing the native SMART interface; USB
bridges and ATA-specific wear attributes are not decoded. Existing `Metrics`, `pipe`, `/json`,
Prometheus and saved preferences retain their formats.

### Output format

The `pipe` command and the HTTP `/json` endpoint return the same metrics. In a macOS VM (`"vm": true` in the `soc` object of `pipe --soc-info` and `/json`) power and frequencies read 0, temperatures `null` and fans are empty:

<details>
<summary>JSON example</summary>

```jsonc
{
  "timestamp": "2025-02-24T20:38:15.427569+00:00",
  "temp": {
    "cpu_temp_avg": 43.73614, // Celsius, null when no sensor has a valid reading
    "gpu_temp_avg": 36.95167, // Celsius, null when no sensor has a valid reading
  },
  "memory": {
    "ram_total": 25769803776, // Bytes
    "ram_usage": 20985479168, // Bytes
    "swap_total": 4294967296, // Bytes
    "swap_usage": 2602434560, // Bytes
  },
  "fans": [
    { "name": "fan0", "rpm": 999, "max_rpm": 4900 },
    { "name": "fan1", "rpm": 1200, "max_rpm": 5200 },
  ],
  "network": [ // Interfaces that are up, excluding loopback; null if unavailable
    {
      "name": "en0",
      "rx_bytes": 18349147606, // Cumulative received bytes
      "tx_bytes": 1852839321, // Cumulative transmitted bytes
      "rx_bytes_per_sec": 65536.0, // Bytes/s, null without a usable baseline
      "tx_bytes_per_sec": 4096.0, // Bytes/s, null without a usable baseline
    },
  ],
  "cpu_scaled_ratio": 0.036854, // Combined frequency-scaled CPU ratio (weighted by core count, 0–1)
  "cpu_active_ratio": 0.092, // Combined active residency ratio (not frequency-scaled, weighted by core count, 0–1)
  "cpu_tiers": [ // CPU core types from the lowest to the highest: E, P on M1–M4; E, S on M5; P, S on M5 Pro/Max; E, P, S on M6; _ when the core type is unknown, e.g. the single tier of a macOS VM
    {
      "label": "E",
      "freq_mhz": 1100, // Tier frequency
      "scaled_ratio": 0.082656614, // Frequency-scaled ratio (0–1)
      "active_ratio": 0.18, // Active residency (not frequency-scaled, 0–1)
      "cores": [
        { "die_id": 0, "core_id": 0, "freq_mhz": 1600, "scaled_ratio": 0.14, "active_ratio": 0.24 },
        { "die_id": 0, "core_id": 1, "freq_mhz": 1700, "scaled_ratio": 0.12, "active_ratio": 0.2 },
      ],
    },
    {
      "label": "P",
      "freq_mhz": 1800,
      "scaled_ratio": 0.015181795,
      "active_ratio": 0.04,
      "cores": [
        { "die_id": 0, "core_id": 0, "freq_mhz": 2100, "scaled_ratio": 0.05, "active_ratio": 0.08 },
        { "die_id": 0, "core_id": 1, "freq_mhz": 2200, "scaled_ratio": 0.07, "active_ratio": 0.06 },
      ],
    },
  ],
  // Deprecated, use cpu_tiers: ecpu_* is the lowest tier, pcpu_* the highest
  "ecpu_freq_mhz": 1100,
  "ecpu_scaled_ratio": 0.082656614,
  "ecpu_active_ratio": 0.18,
  "pcpu_freq_mhz": 1800,
  "pcpu_scaled_ratio": 0.015181795,
  "pcpu_active_ratio": 0.04,
  "ecpu_cores": [/* cores of the first tier */],
  "pcpu_cores": [/* cores of the last tier */],
  "gpu_freq_mhz": 461, // GPU frequency
  "gpu_scaled_ratio": 0.021497859, // Frequency-scaled ratio (0–1)
  "gpu_active_ratio": 0.09, // GPU active residency ratio (not frequency-scaled, 0–1)
  "cpu_power": 0.20486385, // Watts
  "gpu_power": 0.017451683, // Watts
  "ane_power": 0.0, // Watts
  "all_power": 0.22231553, // Watts
  "sys_power": 5.876533, // Watts
  "ram_power": 0.11635789, // Watts
  "gpu_ram_power": 0.0009615385, // GPU SRAM power, Watts
}
```

</details>

### Active and scaled ratios

`active_ratio` is the share of the sampling interval during which the processor was doing any work. `scaled_ratio` is the same measure adjusted for operating frequency, showing the share of the processor's maximum possible capacity that was used.

For interval $T$, $t_i$ is the time spent doing work at frequency $f_i$, and $f_\text{max}$ is the hardware maximum frequency for the CPU cluster or GPU. The product $T f_\text{max}$ is the theoretical maximum frequency-time the processor could deliver at full load without thermal or power throttling:

```math
R_\text{active} = \frac{\sum_i t_i}{T}
\qquad
R_\text{scaled} = \frac{\sum_i t_i f_i}{T f_\text{max}}
```

CPU ratios are calculated per core and averaged across the cluster. A core that did no work contributes zero; combined CPU ratios average all cores from all clusters:

```math
R_\text{cluster} = \frac{1}{N_\text{cores}}\sum_j R_j
```

Examples:

- Full interval at half the maximum frequency → `active = 1.0`, `scaled = 0.5`.
- Half the interval at the maximum frequency → `active = 0.5`, `scaled = 0.5`.
- One of ten cores working at maximum frequency → cluster `active = scaled = 0.1`.

## 📚 Library usage

`macmon` can be used as a Rust library to collect Apple Silicon metrics in your own applications.

Add it to your project:

```sh
cargo add macmon --no-default-features
```

The default `app` feature enables the `macmon` executable and its terminal UI dependencies. Disable default features when using `macmon` only as a library.

Run the [demo example](examples/demo.rs) from this repository:

```sh
cargo run --example demo
```

Then use the `Sampler` to collect metrics:

```rust
use macmon::Sampler;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut sampler = Sampler::new()?;

    // collect metrics over a 1000ms window
    let metrics = sampler.get_metrics(1000)?;

    println!("CPU power:  {:.2} W", metrics.cpu_power);
    println!("GPU power:  {:.2} W", metrics.gpu_power);
    match metrics.temp.cpu_temp_avg {
        Some(value) => println!("CPU temp:   {value:.1} °C"),
        None => println!("CPU temp:   N/A"),
    }
    println!("RAM usage:  {} / {} bytes", metrics.memory.ram_usage, metrics.memory.ram_total);
    for tier in &metrics.cpu_tiers {
        println!("{}-CPU:      {} MHz  {:.1}%", tier.label, tier.freq_mhz, tier.scaled_ratio * 100.0);
    }

    Ok(())
}
```

`get_metrics(duration_ms)` blocks the calling thread while collecting one IOReport delta over the complete interval. For a UI, server, or async application, create the sampler inside a dedicated worker thread and send the completed metrics back through a channel:

```rust
use std::{sync::mpsc, thread};

use macmon::Sampler;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let mut sampler = Sampler::new().expect("failed to create sampler");

        while let Ok(metrics) = sampler.get_metrics(1000) {
            if tx.send(metrics).is_err() {
                break;
            }
        }
    });

    // Use recv() in a consumer thread or try_recv() in a non-blocking event loop.
    let metrics = rx.recv()?;
    println!("CPU power: {:.2} W", metrics.cpu_power);

    Ok(())
}
```

Creating `Sampler` inside the worker keeps its low-level macOS handles on that thread. In an async runtime, use its blocking-thread facility rather than calling `get_metrics` directly from an executor worker.

## 🤝 Contributing

All contributions are welcome! Feel free to open an issue or submit a pull request.

Run `make check` and `make test` for Rust and HTTP checks. With Node.js installed,
`make test-dashboard` exercises the actual browser script against isolated DOM, clock and
request fixtures. Node.js is used only for these development tests.

The [upstream dev pre-release](https://github.com/vladkens/macmon/releases/tag/dev), built from upstream `main`, shows its git version in `macmon -V` and the TUI title, e.g. `macmon 0.9.0-12-g3672a1b` (local builds show the crate version); mention it when reporting an upstream issue. It does not include this fork's Network or browser additions.

## 📝 License

Distributed under the [MIT License](LICENSE).

## 🔍 See also

- [tlkh/asitop](https://github.com/tlkh/asitop) – The original tool. Written in Python, requires sudo.
- [dehydratedpotato/socpowerbud](https://github.com/dehydratedpotato/socpowerbud) – Written in Objective-C, sudoless, no TUI.
- [op06072/NeoAsitop](https://github.com/op06072/NeoAsitop) – Written in Swift, sudoless.
- [graelo/pumas](https://github.com/graelo/pumas) – Written in Rust, requires sudo.
- [context-labs/mactop](https://github.com/context-labs/mactop) – Written in Go, requires sudo.

## Maintaining this personal fork

`Spacgithub/macmon` keeps personal features, including Network Monitoring once its feature PR is
merged. `origin` points to this fork; `vladkens/macmon` is a read-only `upstream`. Upstream fixes,
performance improvements and new features are merged into the fork's history, preserving personal
commits. Changes to the same code can still require conflict resolution and testing.

With Git and authenticated GitHub CLI (`gh auth login`), configure a fresh clone once:

```sh
git remote add upstream https://github.com/vladkens/macmon.git
git remote set-url --push upstream /dev/null
git config --local remote.pushDefault origin
gh repo set-default github.com/Spacgithub/macmon
```

To prepare an update, install the Rust tools used by `make check` / `make test`, including nightly
rustfmt, then run:

```sh
make upstream-sync
```

The command validates every effective remote URL, fetches both main branches, and prepares a merge
in a separate `.worktrees/upstream-sync-…` checkout. It runs the checks before committing, pushes
only the candidate branch to `Spacgithub/macmon`, and opens a PR there. Your current checkout and
main branch stay intact. Repeating the command returns an existing open sync PR, or reports that
upstream is already integrated. Unmerged personal feature branches are not included automatically.

Review the sync PR and choose **Create a merge commit** when merging it. This retains upstream
ancestry, so later updates continue from the previous sync. Personal feature PRs can still use
squash; sync PRs must not use squash or rebase. There is no automatic main update or recurring job.

On a conflict or failed check, the command publishes nothing and prints the retained worktree path.
Resolve the candidate there, stage the resolution, run `make check` and `make test`, and commit the
merge. Rerun `make upstream-sync` to publish a clean candidate with the expected merge parents.
It refuses to overwrite dirty or incomplete existing candidates. Review upstream workflow changes
as well as source changes: the inherited crate and Homebrew publishing jobs are restricted to the
original author's repository; tests continue to run in this fork.

Run `make test-upstream-sync` for local Git fixture tests; they do not contact or publish to GitHub.
