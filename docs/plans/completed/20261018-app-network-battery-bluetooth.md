Done: 2026-10-19, PR #38

# Application network, battery and Bluetooth monitoring

## Agreed scope

Implement three independent feature PRs in `Spacgithub/macmon`: application network
traffic, battery monitoring, and Bluetooth device monitoring. The person explicitly
approved complete Terminal/Web integration and expanding the approximately 200
production-line budget for each feature. Follow the repository's existing review,
checks and PR process; final merge remains with the person.

Each feature uses the shared sampling owner, supports concise summaries and expanded
details, and preserves existing functionality, rootless operation, the library API,
legacy JSON/Prometheus outputs and released settings. Add new telemetry through the
additive sample API with explicit availability and freshness. Process/device names
and identifying details require the same local access boundary as process telemetry.

Menu bar development and fan control are outside this plan. Fan research and the
future independent Swift control system belong to a separate project.

## Decisions

- Keep per-interface network observation and application accounting as distinct views
  with their own accounting scope; do not imply their totals must match under VPN,
  proxy, loopback or shared system-service traffic.
- Reuse confirmed application/helper-process ownership. Show application receive/send
  rates, totals accumulated during the monitoring session, sorting, and expandable PID
  details. Do not label session totals as lifetime or boot totals.
- Use one bounded, supervised process-network acquisition worker per shared owner.
  Collection errors, process replacement, counter decreases and sampling restarts must
  reset rate baselines without spikes. Do not turn unknown data into zero.
- Battery is monitoring only: percentage, power source, charging state, health, cycles,
  and source-supported temperature, charge/discharge power and time estimates. State
  their units and distinguish battery electrical power from chip/system estimates.
- Prefer native power-source notifications and low-frequency health acquisition.
- Bluetooth shows connected devices, type, available battery components and freshness.
  Left/right earbud and case readings are separate when supported. Missing device
  coverage is explicit; do not claim universal Bluetooth battery support.
- Prefer event-driven or cached low-frequency peripheral acquisition. Slow or failing
  peripheral APIs must not block system sampling or create duplicate samplers.
- Keep hardware writes, charging policy, pairing changes, new root helpers and changes
  to existing fan-management applications outside all three monitoring features.

## Tasks

- [x] Validate native sources and document measured coverage, permissions, units and
      unsupported cases on the person's concrete Mac model and macOS release.
- [x] Implement application network acquisition, reset-safe rate/session accounting,
      confirmed grouping, additive sample output and local privacy protection.
- [x] Add Terminal/Web application summaries, expandable members and sorting without
      reducing existing per-interface access.
- [x] Add focused parser, identity, failure, rate, privacy, freshness and UI regressions;
      run checks and runtime/resource validation; review and deliver the network PR.
- [x] Implement battery acquisition, honest availability/freshness and shared output.
- [x] Add Terminal/Web battery summaries and details with focused regressions, native
      runtime verification and resource measurements; review and deliver the battery PR.
- [x] Implement Bluetooth device/battery acquisition with explicit supported coverage.
- [x] Add Terminal/Web device summaries and expandable supported battery components;
      test empty, missing, stale, disconnected and reconnect cases.
- [x] Run checks and real-device/resource validation, independently review and deliver
      the Bluetooth PR. Update the roadmap and complete this plan in the finishing PR.

## Result

Delivered application network monitoring in PR #36, battery monitoring in PR #37,
and connected Bluetooth monitoring in PR #38. Terminal and Web use one shared owner;
legacy outputs, library types, older clients and released settings remain compatible.
Independent source/diff reviews, complete checks, native API/PTY/browser validation
and paired resource measurements are recorded in
[monitoring validation](../../monitoring-validation.md). Unsupported readings remain
unavailable and unmeasured hardware/OS cases are explicitly identified.
