Done: 2026-10-14, PR #12

# Local browser dashboard

## Behavior and decisions

- Add a responsive, self-contained local dashboard at `/dashboard/`, served by the existing
  Rust sampler. Preserve the terminal UI, `/`, `/json`, `/metrics`, CLI defaults, library API
  and saved configuration. Recommend an explicit loopback host for local use.
- Show actual CPU/GPU activity, RAM, history, per-interface Network rates/totals and reported
  fan identities/RPM. Unknown readings remain unavailable; absent hardware is not invented.
- Use restrained dark-blue surfaces, compact mechanical seven-blade fans, careful lighting
  and independent browser animation. Respect reduced motion, visibility, zero RPM and a
  manual animation switch. Keep CPU/GPU, memory and Network readable on a narrow screen.
- Add sample sequence and successful-sample timing through an additive endpoint. Poll with
  one request at a time and clearly show waiting, stale and disconnected states.
- Use no new dependencies or external assets. The person approved an expanded 350–500-line
  feature budget and one independent PR. Retain existing terminal improvements in the tested
  installed binary; only the personal fork receives changes.

## Tasks

- [x] Add compatible HTTP routes and reliable successful-sample metadata.
- [x] Implement the responsive dashboard and compact material fan animation.
- [x] Verify real data, browser layouts, missing data and connection/motion behavior.
- [x] Run complete checks and review the independent personal-fork PR.
- [x] Refresh and verify the installed command with a retained working backup.

## Result

The bundled dashboard displays real CPU/GPU activity, memory history, every reported network
interface and compact material fans. Successful-sample metadata supports waiting, live, stale
and offline states; independent browser motion respects RPM, visibility and user preferences.
Rust, HTTP and frontend checks and actual desktop/narrow-browser verification passed. The
installed command includes both this feature and PR #11, with its previous binary retained.
PR #12 uses PR #11 as a temporary base and remains draft until that dependency is merged.
