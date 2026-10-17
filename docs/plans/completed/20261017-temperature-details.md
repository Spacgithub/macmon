Done: 2026-10-17, PR #19

# Detailed temperature readings

## Behavior and decisions

- Preserve the current CPU/GPU averages and their existing SMC/HID source policy.
- Retain valid CPU/GPU sensor names, group, source and Celsius from the same completed
  sample, without adding sensor reads or guessing physical core, SSD or battery mappings.
- Add a read-only sampler accessor and a `temperatures` array to `/api/sample` for the
  terminal and browser detail views; leave existing public metric structs, pipe, `/json`
  and Prometheus output compatible.
- Missing data stays unavailable, never zero temperature. Failed samples clear detail
  caches so stale readings cannot be published as current.
- The person approved complete temperature and dual-interface detail controls in three
  PRs, with an expanded production-line budget for these features.

## Tasks

- [x] Retain named temperature readings with source and group, with focused tests.
- [x] Publish same-sample temperature details without breaking existing outputs.
- [x] Verify real rootless readings, compatibility, checks and independent review.

## Result

Named CPU/GPU readings are retained and exposed without additional hardware reads or changes
to existing metric structs and legacy outputs. Real rootless M2 Max / macOS 15.6.1 sampling,
compatibility tests, full checks and independent review passed.
