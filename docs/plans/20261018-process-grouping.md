# Application process groups in Terminal and Web

## Behavior and decisions

- Show one application summary with its confirmed helper processes. Each application can be
  expanded independently to inspect the individual processes. Provide an explicit switch
  between application groups and the existing per-process list in both interfaces.
- Prefer grouped summaries when application metadata is available. Use application/bundle
  identity and verified operating-system ownership relationships, separated by user. Keep
  ambiguous or unavailable identities as individual processes; process names alone and
  arbitrary shell descendants do not establish application ownership.
- Validate optional OS ownership metadata before using it for standalone XPC helpers;
  unsupported layouts, unreadable metadata or competing application owners fall back to
  individual rows. Keep collection bounded and cache only live process/application identities.
- Compute totals once per sampled process set. CPU remains 100% per core. Describe memory
  as summed process memory, not deduplicated physical application memory. GPU sums describe
  process shares, not a new device-utilization reading. Missing or partial values remain
  distinguishable from measured zero; guard overflow and duplicate membership.
- Keep every member reachable, including helpers that do not repeat the application's name.
  Group names and member names can be searched. In application mode, summaries remain whole-
  application totals; label matching/total members when filtering the visible children.
  Retain the current raw-list filter, term totals and sort behavior.
- Preserve stable expansion, selection and scroll state during refresh. Forget departed
  identities and distinguish PID reuse. Sort groups by their totals and members by their
  own measurements, with deterministic ties.
- An application summary is not an actionable PID. Process information, files, Finder,
  copying and termination continue to target an explicitly selected real process. Do not
  add a whole-application termination action.
- Web supplies accessible per-group controls with responsive member rows. Terminal uses
  keyboard/mouse controls that preserve filtering, sorting, help, focused panels and narrow
  layouts. Panel-level expansion and application-level expansion remain distinct controls.
- Terminal uses `g` for applications/processes and Left/Right for a selected application's
  collapse/expand; a child can return to its application with Left. Keep Enter and the panel
  shortcuts unchanged. Compact layouts show five root entries rather than five mixed tree
  rows; detailed lists make every expanded member reachable.
- Remember the view mode with compatible additive preferences. Keep individual application
  expansion only in session/page memory, pruning departed identities without persisting PIDs.
- Keep collection rootless, add no dependency, preserve existing Metrics, library APIs,
  pipe/legacy JSON, Prometheus and released preference fields. Extend the loopback-only
  dashboard snapshot additively for grouping, retaining the existing processes array and
  its age/privacy rules. Do not publish executable paths or command lines to the dashboard.
- Deliver three dependent PRs in the personal fork: shared identity/totals and snapshot
  support, Web, then Terminal. Keep the installed program and formal service on merged main
  until the feature PRs are reviewed and merged.

## Tasks

- [x] Validate application/helper attribution against verified macOS metadata and real local
  application processes, including unreadable, ambiguous and restarted processes.
- [x] Implement shared group identity, unique membership, totals and compatible loopback
  dashboard metadata with focused regression tests.
- [ ] Add Web grouped/raw views, independent expansion and refresh/accessibility regressions.
- [ ] Add Terminal grouped/raw views, navigation, filtering and safe real-process actions.
- [ ] Complete focused and full checks, independent reviews and real rootless private-PTY
  and browser validation; retain disk, Network, temperature, pressure/Swap and panel controls.
