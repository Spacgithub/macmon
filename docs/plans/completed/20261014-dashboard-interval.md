Done: 2026-10-14, PR #14

# Live dashboard sampling interval

## Behavior and decisions

- Add a compact web control for the real service sampling interval, using the terminal's
  250–10000 ms bounds and adjustment rules. Show the server's effective setting and pending
  or failed changes honestly, without restarting the service.
- Coordinate system, temperature, Network and process sampling on the service cadence;
  keep fan animation independent. Synchronize freshness, polling and footer text.
- Restrict mutation to intentional same-origin loopback requests. Preserve old read-only
  API formats, CLI defaults, library API and saved terminal settings; add no dependencies.
- Keep this increment within about 200 production lines and a separate personal-fork PR.
  Preserve existing dashboard/terminal work and do not merge any PR.

## Tasks

- [x] Implement bounded shared service cadence and a local control endpoint.
- [x] Add effective interval, pending/error feedback and adaptive browser polling.
- [x] Verify actual cadence changes, invalid requests and recovery.
- [x] Run complete checks and review the personal-fork PR.
- [x] Refresh and verify the installed command with a working backup.

## Result

Web controls adjust the real shared sampling window with effective and pending feedback.
Boundary, request-validation, frontend and real-hardware cadence checks passed, including
browser adjustments and restoration to 1000 ms. The installed preview retains a working backup.
