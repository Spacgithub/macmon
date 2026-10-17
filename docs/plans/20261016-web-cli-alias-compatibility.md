# Existing executable aliases in web controls

## Behavior and decisions

- Reuse an existing desktop service configured through an absolute executable alias
  when that path and the native command refer to the same file.
- Preserve the configured path and plist bytes, login preferences, exact loaded-job
  source, arguments, optional Program and listener ownership checks.
- Reject different executables and unsafe aliases without controlling their jobs.
- Keep metric formats, library API, dependencies and user configuration unchanged.

## Tasks

- [x] Add alias-first lifecycle and foreign-path regression coverage.
- [x] Reconcile verified aliases in the shared controller.
- [ ] Run complete checks, package verification and independent review for PR #18.
