Done: 2026-10-14, PR #3

# First upstream synchronization with Network Monitoring preserved

Merge the person's PR #1 and PR #2 into `Spacgithub/macmon`, update local main, and complete
the first upstream synchronization. `vladkens/macmon` remains read-only; all PRs, pushes and
merges belong to the personal fork.

## Behavior

- Bring the reviewed Network Monitoring and maintenance tool into fork main.
- Fetch upstream main and integrate its VM display improvements through the fork's sync PR.
- Preserve both histories, the per-interface sampler, TUI table, JSON/Prometheus exports,
  fork publication guards and configuration compatibility.
- Resolve overlapping layout changes by reserving network rows before applying the VM's
  power-row policy. Test VM/network combinations and ordinary Mac behavior.
- Run the required checks, rootless runtime validation and independent review before merging
  the sync PR with a merge commit. The person explicitly authorized this first sync's merge.
- Fast-forward local main to the completed fork main and verify a repeat sync is a no-op.

## Tasks

- [x] Verify and merge personal PR #1/#2; update local main.
- [x] Resolve the sync candidate while preserving network and upstream behavior.
- [x] Run build, focused/full tests, rootless runtime checks and independent review.
- [x] Publish and merge the checked sync PR in the personal fork with both histories intact.
- [x] Update local main, verify repeat synchronization and clean owned temporary worktrees.

## Result

PR #1 and PR #2 were squash-merged in `Spacgithub/macmon`; PR #3 merged upstream history
with a merge commit. Network Monitoring remains available in the library, TUI, JSON and
Prometheus, alongside upstream's VM improvements. Local main matches the fork, a repeat
`make upstream-sync` reports no new changes, and all owned temporary worktrees were removed.

Validation passed: `make check`, 193 Rust tests and 14 synchronization fixture groups,
release build, rootless sampling on the M2 Max with macOS 15.6.1, JSON/Prometheus exports,
three terminal sizes, independent code review and PR CI. VM behavior was covered by tests;
a real macOS VM was not available for runtime validation.
