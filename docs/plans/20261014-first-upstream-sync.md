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
- [ ] Publish and merge the checked sync PR in the personal fork with both histories intact.
- [ ] Update local main, verify repeat synchronization and clean owned temporary worktrees.
