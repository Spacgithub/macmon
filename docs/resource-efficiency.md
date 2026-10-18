# Resource efficiency validation

## Sampling and rendering

Terminal and `serve` share a temporary, same-user collector over private local IPC.
The collector owns hardware counters and one complete process snapshot. Consumers
retain independent presentation settings. Closing the last consumer releases the
collector; a running web backend remains a system-metric consumer.

Process demand changes collection frequency, not the set of PIDs. Summary and
Details collect the full process set so rankings and application totals retain
their meaning. Selected-PID file and socket reads run on a separate worker only
while its overlay is open. Fan FPS changes animation scheduling independently of
metric intervals.

## CPU and memory method

The matched comparison uses release builds on an Apple M2 Max (Mac14,5), macOS
15.6.1, UID 501, without sudo. The baseline is fork main `e63c377`. Its installed
binary is retained throughout validation. Each candidate uses a private runtime
directory, a separate loopback HTTP port and a 120 by 60 cell PTY. User preferences
are checked by hash before and after every run.

Run the baseline and candidate together under the same ambient load. Each has a
Terminal consumer and one HTTP backend in separate before/after runtime namespaces.
Request one genuine `/api/sample` per second from each backend. Use a 1000 ms system interval, warm up before measurement,
and measure three bounded rounds per scenario. Compare the median and range of
those rounds. Process-detail scenarios use the opt-in real-hardware TUI fixture
with no settings file and a fake process-action backend; metric and process
collection remain real. No process signals, clipboard or Finder actions are tested
on other applications.

Read native `proc_pid_rusage(RUSAGE_INFO_V2)` once per second for every target PID.
CPU is the elapsed user plus system CPU-time delta, including completed helper
children, divided by monotonic wall time. Convert Mach ticks with
`mach_timebase_info`; 100 percent means one fully occupied core. The candidate
includes its separate collector in both CPU and memory totals. Completed-child
accounting is only added after reaping, so a live collector is counted separately
without double counting. Helper work spanning a measurement boundary can shift
CPU between rounds; compare repeated ranges rather than treating tiny differences
as a gain. Reject a run if a target PID exits or changes its lifetime, HTTP
polling fails, process rates remain unknown, or system/process data stops updating.
The candidate must report ready process collection at the requested demand and
interval, with an independently advancing process sequence. The baseline predates
that metadata, so it is checked through its real process age and known CPU rates.

Report mean sampled RSS and physical footprint as MiB. Aggregate memory is the
sum of the resident processes' readings, with no system-wide deduplication of
shared pages or short-lived helper memory peaks.
Paging and allocator behavior can move RSS substantially; overlapping ranges or
opposing RSS/footprint changes do not establish a memory saving. The observer's
CPU and the browser renderer are excluded from both versions. These results
measure Terminal, the HTTP backend and the collector, rather than total browser
or whole-system power consumption.

## Matched results

The complete stack in PR #35 was compared with main `e63c377` in three 30-second
rounds per case after warmup. Values are medians, with the minimum–maximum of the
three round means in parentheses. CPU uses a single-core 100 percent scale.
All measured windows passed HTTP, process-demand, cadence and freshness checks.
An asynchronous Summary-to-Details transition is allowed during warmup; measured
windows require ready collection at the exact requested demand and interval.

| View | CPU before (%) | CPU after (%) | Change of medians |
| --- | ---: | ---: | ---: |
| Overview | 6.26 (5.55–6.30) | 2.14 (2.13–2.36) | -65.9% |
| Processes expanded | 5.94 (5.89–6.05) | 4.48 (4.34–4.53) | -24.5% |
| Cooling expanded (60 → 30 FPS) | 6.33 (6.02–7.31) | 2.28 (2.23–2.78) | -63.9% |

Overview and Cooling requested 5-second process summaries in the candidate;
Processes requested 1-second details in both versions. The old backend always
collected processes at 1 second. Cooling compared the old fixed 60-FPS target
with the new default 30, using the same complete ANSI text-fan fallback in both
PTYs. These results measure the combined changes.

| View | RSS before (MiB) | RSS after (MiB) | Footprint before (MiB) | Footprint after (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Overview | 45.5 (42.5–47.0) | 68.7 (58.2–71.6) | 34.1 (31.4–35.6) | 52.4 (45.8–53.1) |
| Processes expanded | 48.8 (45.6–50.9) | 76.6 (67.9–80.2) | 36.6 (33.5–38.7) | 53.5 (48.7–55.1) |
| Cooling expanded (60 → 30 FPS) | 56.3 (52.2–57.5) | 75.0 (67.0–78.0) | 36.2 (34.8–37.1) | 48.2 (47.4–48.7) |

CPU fell in all three measured cases. Total RSS increased by about 19–28 MiB;
physical footprint increased by about 12–18 MiB. Both include the extra collector
process and IPC caches. Total memory was higher in these runs. RSS moved
more than footprint during warmup/paging; neither column is deduplicated system memory. The result applies to these paired workloads on the stated host.

## Runtime checks

Use only private test ports and `MACMON_RUNTIME_DIR` namespaces. Confirm that two
consumers report the same collector PID and session, interval changes propagate,
one consumer can close without interrupting the other, an owned test collector
can restart, and the last lease removes its socket and releases its owner lock.
Rootless `pipe` validation remains separate from IPC and browser checks.

Native text-backend animation submissions can be checked with the ignored
`tui::tests::fan_preview::native_graphical_fan_fixture` test and
`MACMON_FAN_PREVIEW_FPS=15|30|60`. The fixture identifies its data as synthetic and
hardware sampling as disabled. Check active frame timestamps, a stable paused
window, resume, complete termios restoration, cursor/alternate-screen/focus/mouse
cleanup, and unchanged user configuration. Submission rate is not a claim about a
terminal emulator's physical display refresh rate.

On the validation host, real API snapshots advanced every 5.028 seconds in Summary
and 1.028 seconds in Details (three and four snapshots respectively, at least 541
PIDs each). Expiring managed and legacy browser leases stopped process updates
while core samples continued. Retained process ages advanced, resumed rates warmed
up, and live application/member identities survived the pause.

The Terminal passed 25 private-PTY interaction checks: grouped/individual views,
independent expansion, filtering, a native selected-PID file read and manual
refresh, resize, FPS controls and complete terminal restoration. The real browser
passed grouping, filtering/sorting, independent application expansion, all eight
panel switches, 5-second/1-second process-state transitions, FPS persistence and
manual fan pause. Network, temperatures, memory pressure, thermal state and disk
health stayed available; independent rootless `pipe` sampling also passed.

The synthetic text-fan fixture submitted about 14.7, 30.1 and 60.3 frames/second
for the selected 15/30/60 targets. Each paused window had zero animation frames;
resume, termios, cursor, alternate screen, focus/mouse modes and image cleanup
passed. These are scheduler submissions, not physical Warp/browser display FPS.
Browser renderer CPU was not included in the native process benchmark.

Other Apple Silicon models, virtual machines and macOS versions need their own
runtime checks. Successful builds and unit tests do not substitute for them.
