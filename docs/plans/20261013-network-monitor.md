# Per-interface network monitoring

Add the requested per-interface network view to the personal fork, `Spacgithub/macmon`.
The feature uses a `feat/network-monitor` branch and a pull request in that fork.

## Behavior

- Sample the macOS public interface MIB without sudo, using its 64-bit receive and transmit
  byte counters. Include interfaces that are up; exclude loopback. Keep VPN and bridge
  interfaces separate because summing them can count traffic more than once.
- Show each interface's receive/transmit bytes per second and cumulative receive/transmit
  bytes. Rates use the measured interval between snapshots, including time between calls.
  Seed a baseline at the beginning of the first sampling interval. New interfaces and
  decreased counters have unavailable rates until a new baseline is established.
- Network collection errors do not interrupt CPU, GPU, or memory collection. Report the
  unavailable network snapshot explicitly, clear its baseline, and retry on the next sample.
- Add a compact full-width TUI panel with byte units, fitting rates and totals at 80 columns;
  omit whole total columns on smaller screens and indicate when only some interfaces fit.
  Show the busiest interfaces first; JSON and Prometheus include every sampled interface.
- Extend the Rust library and JSON additively with per-interface network metrics. Add
  Prometheus byte counters and bytes-per-second gauges with escaped interface labels.
  Keep existing metric names and values, CLI behavior, and released configuration fields.

## Decisions

- Use the existing `libc` dependency and public `IFMIB_IFALLDATA` / `IFDATA_GENERAL` sysctl.
  Do not use `getifaddrs`' 32-bit counters or a subprocess for normal metric collection.
- Cumulative bytes are the kernel's interface totals, which can reset when an interface
  restarts; they are not totals since macmon started.
- No per-process network monitoring or new configuration options in this feature.
- Aim for approximately 200 production Rust lines, excluding tests; simplify after review
  if the implementation exceeds the project's feature budget.

## Tasks

- [x] Add rootless 64-bit interface collection and focused source tests.
- [x] Integrate measured rates, baseline recovery, resets, and public/JSON metric types.
- [x] Add the TUI network panel and focused narrow/short terminal rendering tests.
- [x] Add Prometheus counters/gauges, compatibility tests, and user documentation.
- [x] Run `make check`, `make test`, and rootless `pipe`/HTTP/TUI runtime validation.
- [ ] Review the integrated diff and open the pull request only in the personal fork.
