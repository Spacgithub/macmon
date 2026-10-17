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
- 💾 RAM / Swap usage
- 🌐 Per-interface network receive/transmit rates and cumulative traffic
- 📈 Historical charts with average and max values
- 🌡️ Average CPU / GPU temperature
- 📋 Process list with per-process CPU, memory, power and GPU usage
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

Run `macmon` without a subcommand to open the terminal UI. At least 100 columns and 30 rows
show Compute on the left, Memory and a Power/Temperature ledger on the right, and full-width
Network and Processes below. Smaller windows and VMs use the adaptive metric grid. Press `?`
in the app to see the keys.

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
absent. Press `a` to toggle animation (enabled by default, saved as `animate_fans`). Complete fans
target 60 updates per second independently of metric/process sampling; compact indicators
change at most four times per second. Animation pauses on focus loss, under help and when
hidden, preserving your manual choice. It never controls hardware fans or changes the saved
sampling interval (1000 ms by default).

The screen redraws when data, input, the window size or a visible fan frame changes. Other input
heartbeats skip drawing, and interfaces are ranked when samples arrive. Existing configuration
fields, including `Sparkline` and `Gauge`, remain compatible.

For the optional deep blue Warp palette, copy [macmon-deep-space.yaml](themes/macmon-deep-space.yaml)
to `~/.warp/themes/`, then select **Macmon Deep Space** in Warp's Theme Picker
(`Ctrl+Cmd+T`). See [Warp's custom theme guide](https://docs.warp.dev/terminal/appearance/custom-themes/).
The program uses ANSI colors and also follows other terminal palettes; it does not change your
terminal preferences automatically.

Press `/` to filter the process list. A filter is a comma-separated list of terms, each matched case-insensitively against the process name or pid: `safari, cargo` shows processes matching any term, and a term starting with `!` hides its matches (`chrome, !helper`). `Enter` keeps the filter, `Esc` clears it.

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
| `GET /dashboard/` | HTML | Local browser dashboard with CPU/GPU activity and temperature, memory, Network, cooling and Processes |
| `GET /api/sample` | JSON | Successful-sample sequence, timestamp, age, interval, service cadence, metrics and named temperature readings; local-only process snapshot |
| `POST /api/interval` | JSON | Adjust real service sampling with `{"adjustment":"increase"}` or `"decrease"`; local same-origin requests only |

#### Local dashboard

Run `macmon serve --host 127.0.0.1`, then open
[http://127.0.0.1:9090/dashboard/](http://127.0.0.1:9090/dashboard/).
The dashboard is bundled into the binary and uses no external assets or frontend build step.
It follows the service sampling interval and retains 60 distinct samples. CPU/GPU activity uses
active residency ratios. Each panel starts with a compact summary; use its **Details** / **Less**
button or **Expand all** / **Collapse all** to choose how much to show. These choices are saved
locally in the browser and remain usable when browser storage is blocked. Summaries keep the
three busiest interfaces and five processes in the selected sort order; expand to scroll
through all rows. Folding a panel keeps live sampling and summaries running.

The Power panel separates the system estimate from the CPU + GPU + ANE subtotal and plots
both on a shared watt scale over the latest 60 distinct samples. CPU, GPU, ANE, DRAM and GPU
SRAM readings retain small positive values in milliwatts. The system estimate uses the larger
of the system sensor reading and the CPU + GPU + ANE subtotal; it is not wall-socket power.
Memory readings are separate and should not be added to either total. Missing, invalid and
virtual-machine power stays unavailable; system zero is unavailable. Component zero remains
a reported value because unsupported counters can also report zero.

Named CPU and GPU temperature readings show Celsius or `Unavailable`. The Processes table
includes names/PIDs, CPU, GPU and memory, sortable by usage; CPU 100% means one busy core.
New processes and unavailable rates show a dash. Process sampling runs once per completed
metrics cycle, independently of browsers. Only loopback clients with a matching loopback Host
receive process names and PIDs; other requests get `processes: null`. Snapshot age is reported
as `processes_age_ms`, and all processes remain accessible in the scrollable table.
System metric timestamps and age include the time spent collecting the separate process snapshot.

`/api/sample` also includes a top-level `temperatures` array from the same successful hardware
sample, for example `{"name":"Tp01","group":"cpu","source":"smc","celsius":45.0}`.
Each entry retains its original sensor name, the existing CPU/GPU group and SMC/HID source.
SMC grouping is heuristic: these names do not establish physical core, battery or storage
locations. Only finite readings above 0 and at most 150 °C are retained; missing detail is an
empty array. Library callers can use `Sampler::temperature_details()` after `get_metrics()`;
the accessor performs no hardware reads and is empty before sampling or after a failed sample.
Existing `Metrics`/`TempMetrics` structs, `pipe`, `/json` and Prometheus formats are unchanged.

Expand **Temperature & cooling** for each group's current sensor count, minimum, maximum
and hottest raw sensor name, the full source-labelled sensor list, and a 60-sample CPU/GPU
mean-temperature history. History peaks describe recorded group means; current sensor maxima
describe individual readings. Missing values leave gaps. The compact view keeps means and
fan RPM visible, while folding Cooling pauses its fan animation.

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
readings and VM-only placeholders stay unavailable. The Fan motion switch and reduced-motion
preference control browser animation independently of sampling. Hidden pages pause polling
and animation. WAITING, LIVE, STALE and OFFLINE describe connection/sample state while keeping
the last available values visible. `/api/sample` adds freshness metadata; `/json`, `/metrics`,
the root JSON response and existing server defaults retain their formats.

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
interface that is up, excluding loopback. The compact panel shows the busiest interfaces;
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
