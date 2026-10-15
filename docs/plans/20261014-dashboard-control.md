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
- [ ] Verify repeat start/stop, KeepAlive, foreign-job rejection and separate TUI survival.
- [ ] Install and check the user-facing desktop control without changing login preferences.
- [ ] Run complete checks and review the personal-fork PR.
- [ ] Refresh and verify the installed command with a working backup.
