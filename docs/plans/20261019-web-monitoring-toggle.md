# Manual Web monitoring switch

## Behavior and decisions

- Add a monitoring on/off switch in the dashboard header, with an accessible label,
  explicit enabled/paused/reconnecting feedback and protection against repeated clicks.
- Off releases this HTTP backend's shared-source subscription, including system and
  process demand. Retain the lightweight HTTP control page so On works in the same tab.
  A collector with no other consumer exits after its existing idle grace; independent
  Terminal or other Web consumers continue sampling normally.
- On reconnects to the shared source and waits for a fresh reading. Preserve the last
  shared cadence without replaying an old interval over a running Terminal consumer.
  Guard the connect/handshake/frame paths against late results from an earlier state.
- Keep the switch server-wide for all tabs of this backend. Refresh its state while
  paused without requesting metrics or process snapshots; stop browser animation and
  label retained values explicitly. A slow or failed request must not report a confirmed
  state change, revive old samples or silently enable monitoring.
- Add bounded monitoring GET/POST controls. Mutations reuse the existing same-origin,
  loopback-peer and Host checks. Preserve protocol detection and static/control routes
  while paused. Data endpoints return unavailable while paused rather than presenting
  cached JSON/Prometheus readings as current measurements.
- Start each server enabled, preserving existing unattended monitoring behavior.
  Keep `macmon web start/stop/status` as service controls: full stop still terminates
  the managed job and releases its port. Document the distinction from the Web switch.
- Preserve released preference fields, legacy output schemas, the library API,
  rootless behavior, process grouping, network/disk health and existing dashboard panels.
  Add no dependency. Deliver through one feature PR in Spacgithub/macmon only.
- Validate candidate builds using private runtime namespaces, test ports and temporary
  clients. Leave the installed binary, managed service and main unchanged until merge
  and installation are separately requested.

## Tasks

- [x] Confirm the switch placement, full behavior and production-code budget.
- [x] Implement client subscription suspension/resumption with focused lifecycle tests.
- [x] Implement guarded HTTP controls and honest paused data availability.
- [x] Add accessible dashboard controls, state synchronization and regression tests.
- [ ] Verify rootless shutdown/resume, a concurrent Terminal consumer, cadence and
      browser behavior; run full checks and independent review, then publish the PR.
