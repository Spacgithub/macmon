Done: 2026-10-13, PR #2

# Preserve fork features while merging upstream improvements

Maintain `Spacgithub/macmon` as the personal fork. Keep Network Monitoring and future personal
features on its main branch after the person merges their feature PRs; merge original-author
updates through a separate, tested PR instead of replacing the fork with the upstream tree.

## Behavior

- `origin` is the personal fork; `upstream` reads `vladkens/macmon`. Disable upstream pushes
  and make `origin` the local default push destination.
- Add a manual `make upstream-sync` command. Validate effective remote URLs, fetch and pin
  both main commits, and exit without a PR when upstream is already integrated.
- Prepare each update in an isolated worktree based on the fork's main commit. Merge upstream
  with both histories preserved, run `make check` and `make test`, then commit and publish
  only the sync branch to the personal fork and open its PR.
- Preserve dirty user checkouts. On conflicts or failed checks, stop publication and retain
  the candidate for review. Detect an existing open sync PR and avoid duplicate publication.
- Sync PRs use a merge commit when the person merges them, preserving imported upstream
  ancestry. Ordinary feature PRs may still use squash. No reset, forced main update, or
  automatic merge of the fork's main branch.
- Keep inherited test jobs active; publication jobs for the original crate and Homebrew tap
  must run only in the original author's repository.

## Decisions

- No recurring scheduler or automatic merge in this implementation.
- Use Git, GitHub CLI, Make, and POSIX shell; add no project dependencies.
- Network Monitoring PR #1 remains subject to the person's final merge. The sync command
  incorporates personal features already merged into fork main, not unmerged feature branches.
- Future upstream workflow changes require review of fork publication guards as well as
  the existing build, test, and relevant runtime checks.

## Tasks

- [x] Configure verified upstream read access and fork-only default pushes.
- [x] Implement isolated merge/check/own-fork PR creation and focused fixture tests.
- [x] Document fork maintenance and the merge-commit exception; guard inherited publishers.
- [x] Verify local fixtures, `make check`, `make test`, and an actual isolated merge preview.
- [x] Review the integrated diff and open the tooling PR only in the personal fork.

## Result

Added manual upstream synchronization through isolated, checked merge PRs in the personal fork.
The tool preserves fork history and local edits, stops unsafe publication, and requires merge
commits for synchronization PRs. Fork maintenance rules and inherited publisher guards are in
place; focused Git fixtures are included in `make test`.
