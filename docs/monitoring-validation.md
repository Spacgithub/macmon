# Application and peripheral monitoring validation

## Host and method

Native validation uses an Apple M2 Max (Mac14,5), 32 GiB RAM, macOS 15.6.1
(24G90), UID 501, without sudo. Each run has its own `MACMON_RUNTIME_DIR`,
loopback HTTP ports and owned PTY. The installed program, production backend and
user preferences are preserved.

For resource comparisons, run two release builds together under the same ambient
load. Each build has a 120×60 Terminal, one Web backend and its own shared
collector. Request `/api/sample` once a second; use a 1000 ms system cadence and
5000 ms complete process summaries. Warm up for 20 seconds, then measure three
30-second rounds. The comparison baseline for the first feature is fork main
`5c2fca6`; later features are compared with the preceding feature stack.

Read `proc_pid_rusage(RUSAGE_INFO_V2)` for every live descendant once a second.
CPU includes user, system and reaped-child CPU time; 100% means one occupied core.
RSS and physical footprint are sums of live process readings, without deduplicating
shared pages. Include `nettop` and reporter children; short-lived helper memory
peaks can fall between readings. The observer and browser renderer are excluded.

Reject measured windows if a required process exits or changes lifetime, polling
fails, system or process sequences stop, cadence changes, or feature acquisition
is unavailable/stale. Network must have independently advancing observations and
known rates. Battery and Bluetooth must retain usable, fresh native reports when
included. Compare medians and ranges; small overlapping differences are noise.

## Application network coverage

- One persistent native `nettop` observer per collector; Terminal and Web share it.
  A private PTY flushes its CSV without changing a user's terminal. Final-build
  testing used no auxiliary network observer.
- The observer uses that private terminal for both input and output. With
  `/dev/null` input, native `nettop` consumed 136–138% of one core despite its
  two-second report cadence. Changing only input to the private terminal measured
  0.52% over four seconds after warmup. A focused child-stdio regression protects
  this behavior. Close the master before reaping the child: retaining it can stall
  the macOS child-exit path while the parent waits.
- Four closed TCP downloads to a dedicated, still-live process transferred
  8,388,608 payload bytes. Observed session RX increased by 8,389,076 bytes,
  including 468 response-header bytes; TX increased by 492 bytes.
- One earlier burst test missed its bounded expected-total deadline. Five traced
  reruns and the corrected final build matched the transfer; the earlier cause
  was not established. Observation gaps and exited/unverifiable processes remain
  outside provable totals, rather than being filled with estimated bytes.
- Real lifetime/UID checks and confirmed bundle/coalition ownership bracket each
  observation. Parser, partial metadata, PID reuse/exec, UID changes, first-seen
  unresolved owners, counter resets, incomplete enumeration, cache bounds and
  recovery cases have deterministic regressions.
- Local access protection, legacy JSON/Prometheus compatibility, independent
  process demand, forced private-owner death, observer HUP cleanup, reconnect and
  last-consumer retirement were exercised with real hardware.
- A real-data Terminal fixture passed interface/application switching, summary/full
  views, independent member toggles, sorting, scrolling and 28×12, 80×24 and
  160×48 resizing. It restored the original termios, cursor, alternate screen,
  focus and mouse modes; the fixture uses no settings file.

These are observed TCP/UDP socket counters, including local, proxy and VPN traffic.
They are separate from per-interface counters. Session totals begin at observation;
they do not measure lifetime, boot or Internet-only usage. Native UDP traffic,
other Mac models and other macOS releases were not independently measured here.

## Battery coverage

- The shared owner uses native power-source notifications with a cached, low-frequency
  fallback. Registry health readings stay separate from power-source acquisition age.
  Battery sampling does not wait on or change the core/process sampling cadence.
- On this Mac, the native source reported 81%, AC power, not charging, Good condition,
  150 cycles, approximately 12.19 V and 0 A / 0 W. The electrical flow is voltage times
  signed current at the battery terminals; it is separate from chip/system estimates
  and wall power.
- Temperature, nominal/design capacity and time estimates were unavailable. They remain
  null in the API and visibly unavailable in details, rather than becoming zero.
  Freshness is acquisition age; it does not establish the hardware's update time.
- Native loopback API checks covered two backends sharing one owner, additive legacy
  compatibility, an installed older Terminal sharing that owner, and complete worker
  retirement. Power summaries/details, stale/missing data and wide/compact layouts
  have regressions. The real Web preview showed the same supported readings.

Charging, discharging, UPS, absent-battery and malformed native-property cases use
fixtures. The validation did not change charging policy, disconnect power, or write
hardware settings. Other hardware/macOS combinations remain unmeasured.

## Bluetooth and combined-client coverage

The native report contained two connected peripherals: a keyboard without a reported
battery, and a M585/M590 mouse with 100% main charge. Missing batteries remain unavailable.
See [Bluetooth source evidence and coverage](bluetooth-monitoring.md) for the bounded
read-only reporter, supported component keys, cache semantics and privacy boundary.
Earbud left/right/case charge, controller-off, disconnect/reconnect and malformed reports
use fixtures; no actual radio, pairing or peripheral state was changed.

The complete stack passed private two-backend/single-owner checks, a released older
Terminal joining the new owner, remote redaction of application/device identities,
legacy output compatibility and complete owner/worker retirement. A real-data,
settings-free PTY additionally exercised application sorting/member expansion,
battery summary/details, Bluetooth access from Power and Network, 28×12 to 160×48
resizing and exact terminal-mode restoration. The browser preview passed independent
member toggles, sorting, all-panel expansion/collapse and both narrow and desktop layouts
with no console warnings/errors. Existing disk, temperature, memory and process panels
continued to receive data. Production services and saved preferences were preserved.

## Matched resource results

Values are medians of three 30-second round means; parentheses show the range.
Every measured window passed freshness, cadence and process-lifetime checks,
including the persistent observer's lifetime. Both builds kept one shared owner
for their Terminal and Web consumers, and retired their test processes afterward.

| Added feature | Reference | CPU before (%) | CPU after (%) |
| --- | --- | ---: | ---: |
| Application network | Main `5c2fca6` | 2.59 (2.50–2.62) | 4.08 (3.90–4.12) |
| Battery | Application-network stack | 3.85 (3.79–4.09) | 3.68 (3.48–3.98) |
| Bluetooth | Application-network + battery stack | 3.85 (3.83–4.10) | 4.13 (4.02–4.40) |

| Added feature | RSS before (MiB) | RSS after (MiB) | Footprint before (MiB) | Footprint after (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Application network | 90.6 (80.2–95.5) | 104.8 (87.0–111.6) | 59.5 (56.9–59.6) | 59.6 (51.1–60.1) |
| Battery | 97.1 (85.3–104.3) | 98.2 (86.5–103.6) | 57.5 (52.2–59.7) | 59.6 (59.4–59.7) |
| Bluetooth | 96.9 (87.5–101.5) | 99.8 (89.8–103.9) | 61.7 (61.4–61.7) | 61.2 (59.1–61.3) |

Application monitoring added 1.49 percentage points of one core and 14.3 MiB
sampled RSS at the medians. Footprint ranges overlap; these runs do not establish
a physical-memory saving. The new observer is included in these totals.

The battery comparison retained fresh native readings in every measured window.
Its CPU, RSS and footprint ranges overlap their reference ranges; this sample does
not establish a resource saving or a meaningful CPU increase. Compare each row only
with its paired reference, rather than adding medians from different runs.

Bluetooth added 0.29 percentage points of one core and 2.9 MiB sampled RSS at the
medians. These small differences are not a general performance guarantee; three
rounds also do not establish a physical-memory saving. Its native reports stayed
fresh, with the 30-second reporter and reaped child CPU included.

An initial Bluetooth comparison was discarded when its reference stack's original
`nettop` PID disappeared while the collector and consumers stayed alive. The native
error and any replacement child's executable were not captured, so the cause is
unknown. A complete rerun passed all three fixed-lifetime/freshness windows; no
required PID was substituted and no part of the failed window entered the results.
Acquisition failures invalidate rate baselines and omit uncertain traffic until
fresh verified observations resume; stable collector IDs alone do not prove
continuous network observation.
