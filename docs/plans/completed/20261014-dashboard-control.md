Done: 2026-10-14, PR #15

# Desktop dashboard service control

## Behavior and decisions

- Provide a lightweight double-clickable macOS control outside the webpage, with explicit
  start/open and stop actions. Starting opens the dashboard; stopping actually stops the
  managed monitoring service and permits starting it again.
- Use built-in system tools and launchd, with no new framework or dependency. Identify the
  managed job precisely; never stop processes by name or affect a separate terminal TUI.
- Stop by removing the loaded job from the user's launchd domain so KeepAlive cannot undo
  the action. Keep login startup preferences and any existing agent plist unchanged.
- Use a loopback-only per-session service when no compatible existing agent is installed.
  Keep this increment within about 200 production lines and a personal-fork PR; no merges.

## Tasks

- [x] Implement precisely scoped start/status/stop actions and a desktop entry installer.
- [x] Verify repeat start/stop, KeepAlive, foreign-job rejection and separate TUI survival.
- [x] Install and check the user-facing desktop control without changing login preferences.
- [x] Run complete checks and review the personal-fork PR.
- [x] Refresh and verify the installed command with a working backup.

## Result

The signed desktop controller starts and stops its verified launchd job without affecting
separate terminal monitors or login preferences. Isolated failure fixtures and real service
lifecycle checks passed; the desktop entry and installed preview retain recoverable backups.
