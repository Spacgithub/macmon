#!/bin/sh
# All launchd, network, browser and app compilation operations are isolated stubs.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd -P)
fixture=$(mktemp -d "${TMPDIR:-/private/tmp}/macmon-control-test.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
trap 'exit 130' HUP INT TERM
mkdir "$fixture/tools" "$fixture/Installed tools"
export FIXTURE_ROOT="$fixture"
export FIXTURE_SOURCE_ROOT="$root"
export MACMON_CONTROL_STATE_DIR="$fixture/state with spaces"
export MACMON_CONTROL_AGENT_PLIST="$fixture/login agent.plist"
export PATH="$fixture/tools:$PATH"
binary="$fixture/Installed tools/macmon's & executable"
cat > "$binary" <<'SH'
#!/bin/sh
[ "$1" = --version ] && printf 'macmon test\n'
SH
chmod +x "$binary"
cat > "$fixture/tools/launchctl" <<'SH'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FIXTURE_ROOT/commands"
case "$1" in
  print)
    if [ "${MOCK_PRINT_FAIL:-0}" = 1 ]; then printf 'XPC connection interrupted\n' >&2; exit 5; fi
    if [ -f "$FIXTURE_ROOT/job" ]; then cat "$FIXTURE_ROOT/job"; exit; fi
    printf 'Bad request.\nCould not find service "com.macmon" in domain for user gui: %s\n' "$(id -u)" >&2
    exit 113;;
  bootout) [ "${MOCK_BOOTOUT_FAIL:-0}" = 0 ] || exit 9; rm -f "$FIXTURE_ROOT/job" "$FIXTURE_ROOT/listener"; exit;;
  bootstrap) [ "${MOCK_BOOTSTRAP_FAIL:-0}" = 0 ] || exit 9; plist=$3;;
  kickstart) plist=$(cat "$FIXTURE_ROOT/source");;
  *) exit 11;;
esac
printf '%s\n' "$plist" > "$FIXTURE_ROOT/source"
previous=$(cat "$FIXTURE_ROOT/counter" 2>/dev/null || printf 1000)
pid=$((previous + 1)); printf '%s\n' "$pid" > "$FIXTURE_ROOT/counter"
{
  printf 'gui/test/com.macmon = {\n\tpath = %s\n\tprogram = %s\n\targuments = {\n' "$plist" "$(plutil -extract ProgramArguments.0 raw "$plist")"
  for index in 0 1 2 3 4 5; do printf '\t\t%s\n' "$(plutil -extract "ProgramArguments.$index" raw "$plist")"; done
  printf '\t}\n\tpid = %s\n}\n' "$pid"
} > "$FIXTURE_ROOT/job"
[ "${MOCK_NO_LISTENER:-0}" = 1 ] || printf '%s\n' "$pid" > "$FIXTURE_ROOT/listener"
SH
cat > "$fixture/tools/lsof" <<'SH'
#!/bin/sh
[ ! -f "$FIXTURE_ROOT/listener" ] || cat "$FIXTURE_ROOT/listener"
SH
cat > "$fixture/tools/curl" <<'SH'
#!/bin/sh
[ "${MOCK_HTTP_FAIL:-0}" = 0 ] && [ -s "$FIXTURE_ROOT/listener" ]
SH
cat > "$fixture/tools/open" <<'SH'
#!/bin/sh
printf '%s\n' "$1" >> "$FIXTURE_ROOT/opened"
SH
cat > "$fixture/tools/osacompile" <<'SH'
#!/bin/sh
[ "${MOCK_COMPILE_FAIL:-0}" = 0 ] || exit 9
[ "$1" = -o ] && [ -f "$3" ]
mkdir -p "$2/Contents/Resources"
plutil -create xml1 "$2/Contents/Info.plist"
if [ -n "${MOCK_TARGET_RACE:-}" ]; then mkdir "$MOCK_TARGET_RACE"; printf 'foreign sentinel\n' > "$MOCK_TARGET_RACE/sentinel"; fi
SH
cat > "$fixture/tools/plutil" <<'SH'
#!/bin/sh
if [ "${MOCK_PLUTIL_FAIL:-0}" = 1 ] && [ "$1" = -insert ] && [ "$2" = ProgramArguments.2 ]; then exit 9; fi
exec /usr/bin/plutil "$@"
SH
cat > "$fixture/tools/codesign" <<'SH'
#!/bin/sh
set -eu
app=$4; resources="$app/Contents/Resources"
case "$1" in
  --force)
    [ "$2" = --sign ] && [ "$3" = - ]
    [ "${MOCK_CODESIGN_FAIL:-0}" = 0 ] || exit 9
    cmp "$FIXTURE_SOURCE_ROOT/scripts/dashboard-control.sh" "$resources/control.sh"
    [ -s "$resources/binary.txt" ]
    [ "$(plutil -extract CFBundleName raw "$app/Contents/Info.plist")" = 'macmon Web Control' ]
    [ "$(plutil -extract CFBundleIdentifier raw "$app/Contents/Info.plist")" = com.macmon.web-control ]
    [ "$(plutil -extract LSUIElement raw "$app/Contents/Info.plist")" = true ]
    cksum "$app/Contents/Info.plist" "$resources/control.sh" "$resources/binary.txt" > "$FIXTURE_ROOT/signed-state";;
  --verify)
    [ "$2" = --deep ] && [ "$3" = --strict ]
    [ "${MOCK_CODESIGN_VERIFY_FAIL:-0}" = 0 ] || exit 9
    cksum "$app/Contents/Info.plist" "$resources/control.sh" "$resources/binary.txt" > "$FIXTURE_ROOT/current-state"
    cmp "$FIXTURE_ROOT/signed-state" "$FIXTURE_ROOT/current-state";;
  *) exit 11;;
esac
SH
cat > "$fixture/tools/sleep" <<'SH'
#!/bin/sh
exit 0
SH
chmod +x "$fixture/tools/"*
control() { sh "$root/scripts/dashboard-control.sh" "$1" "$binary"; }
reject() {
  if "$@" > "$fixture/rejection" 2>&1; then printf 'Expected rejection: %s\n' "$*" >&2; exit 1; fi
}
no_job() { [ ! -e "$fixture/job" ] && [ ! -e "$fixture/listener" ]; }
reset() { rm -rf "$MACMON_CONTROL_STATE_DIR"; rm -f "$fixture/job" "$fixture/listener" "$fixture/commands" "$fixture/opened" "$MACMON_CONTROL_AGENT_PLIST"; }

control status | grep -q '^Stopped'
[ ! -e "$MACMON_CONTROL_STATE_DIR" ]
control start >/dev/null
control status | grep -q '^Running'
[ "$(plutil -extract KeepAlive raw "$MACMON_CONTROL_STATE_DIR/com.macmon.plist")" = true ]
[ "$(stat -f %Lp "$MACMON_CONTROL_STATE_DIR")" = 700 ]
first_pid=$(cat "$fixture/listener")
control start >/dev/null
[ "$(cat "$fixture/listener")" = "$first_pid" ]
[ "$(grep -c '^bootstrap ' "$fixture/commands")" = 1 ]
control open >/dev/null
grep -q '^http://127.0.0.1:19090/dashboard/$' "$fixture/opened"
control stop >/dev/null; control stop >/dev/null; no_job
control start >/dev/null
[ "$(cat "$fixture/listener")" != "$first_pid" ]
control stop >/dev/null; no_job
control start >/dev/null
mv "$binary" "$fixture/temporarily-hidden-binary"
reject control start
control status | grep -q '^Running'
control stop >/dev/null; no_job
control status | grep -q '^Stopped'
mv "$fixture/temporarily-hidden-binary" "$binary"
printf 'PASS repeat on/off, KeepAlive, exact label stop and browser readiness\n'

control start >/dev/null
cp "$MACMON_CONTROL_STATE_DIR/com.macmon.plist" "$fixture/legacy-template.plist"
control stop >/dev/null
cp "$fixture/legacy-template.plist" "$MACMON_CONTROL_AGENT_PLIST"
plutil -remove ProgramArguments.3 "$MACMON_CONTROL_AGENT_PLIST"
plutil -insert ProgramArguments.3 -string 0.0.0.0 "$MACMON_CONTROL_AGENT_PLIST"
plutil -remove ProgramArguments.5 "$MACMON_CONTROL_AGENT_PLIST"
plutil -insert ProgramArguments.5 -string 9090 "$MACMON_CONTROL_AGENT_PLIST"
cp "$MACMON_CONTROL_AGENT_PLIST" "$fixture/preserved.plist"
printf 'disabled-preferences sentinel\n' > "$fixture/Disabled preferences"
control start >/dev/null
control stop >/dev/null
cmp "$fixture/preserved.plist" "$MACMON_CONTROL_AGENT_PLIST"
grep -q '^disabled-preferences sentinel$' "$fixture/Disabled preferences"
printf 'PASS compatible login plist and preferences remain unchanged\n'

reset
printf '9999\n' > "$fixture/listener"
reject control start
[ "$(cat "$fixture/listener")" = 9999 ]
! grep -q '^bootstrap ' "$fixture/commands"
reset; control start >/dev/null
sed 's@program = .*@program = /foreign/executable@' "$fixture/job" > "$fixture/foreign"
mv "$fixture/foreign" "$fixture/job"
reject control stop; reject control start
! grep -q '^bootout ' "$fixture/commands"
reset; control start >/dev/null
sed 's@path = .*@path = /foreign/source.plist@' "$fixture/job" > "$fixture/foreign"
mv "$fixture/foreign" "$fixture/job"
reject control stop
! grep -q '^bootout ' "$fixture/commands"
reset; control start >/dev/null
sed 's@127.0.0.1@0.0.0.0@' "$fixture/job" > "$fixture/foreign"
mv "$fixture/foreign" "$fixture/job"
reject control stop
! grep -q '^bootout ' "$fixture/commands"
printf 'PASS foreign port, executable and source path are never controlled\n'

reset
(MOCK_NO_LISTENER=1 reject control start)
no_job
grep -q '^bootout gui/' "$fixture/commands"
reset
(MOCK_HTTP_FAIL=1 reject control open)
no_job; [ ! -e "$fixture/opened" ]
reset
(MOCK_BOOTSTRAP_FAIL=1 reject control start)
no_job
reset
(MOCK_PLUTIL_FAIL=1 reject control start)
no_job; [ ! -e "$MACMON_CONTROL_STATE_DIR/com.macmon.plist" ]
[ -z "$(find "$MACMON_CONTROL_STATE_DIR" -name '.com.macmon.*' -print)" ]
control start >/dev/null; control stop >/dev/null; no_job
reset
(MOCK_HTTP_FAIL=1 MOCK_BOOTOUT_FAIL=1 reject control start)
grep -q 'Startup cleanup failed' "$fixture/rejection"
[ -e "$fixture/job" ]
control stop >/dev/null; no_job
reset; control start >/dev/null; rm "$fixture/listener"
control status | grep -q '^Loaded, not listening'
(MOCK_PRINT_FAIL=1 reject control stop)
grep -q 'Unable to query' "$fixture/rejection"
[ -e "$fixture/job" ]
! grep -q '^bootout ' "$fixture/commands"
control start >/dev/null
grep -q '^kickstart -k gui/' "$fixture/commands"
control stop >/dev/null; no_job
printf 'PASS failed startup cleanup and loaded-but-not-listening recovery\n'

reset
destination="$fixture/macmon's Web Control.app"
sh "$root/scripts/dashboard-control.sh" install "$binary" "$destination" >/dev/null
cmp "$root/scripts/dashboard-control.sh" "$destination/Contents/Resources/control.sh"
[ "$(cat "$destination/Contents/Resources/binary.txt")" = "$binary" ]
sh "$destination/Contents/Resources/control.sh" status "$binary" | grep -q '^Stopped'
printf 'unrelated app sentinel\n' > "$destination/sentinel"
reject sh "$root/scripts/dashboard-control.sh" install "$binary" "$destination"
grep -q '^unrelated app sentinel$' "$destination/sentinel"
(MOCK_COMPILE_FAIL=1 reject sh "$root/scripts/dashboard-control.sh" install "$binary" "$fixture/failure.app")
[ ! -e "$fixture/failure.app" ]
(MOCK_CODESIGN_FAIL=1 reject sh "$root/scripts/dashboard-control.sh" install "$binary" "$fixture/sign-failure.app")
[ ! -e "$fixture/sign-failure.app" ]
(MOCK_CODESIGN_VERIFY_FAIL=1 reject sh "$root/scripts/dashboard-control.sh" install "$binary" "$fixture/verify-failure.app")
[ ! -e "$fixture/verify-failure.app" ]
race_app="$fixture/race.app"
(MOCK_TARGET_RACE="$race_app" reject sh "$root/scripts/dashboard-control.sh" install "$binary" "$race_app")
grep -q '^foreign sentinel$' "$race_app/sentinel"
[ "$(find "$race_app" -type f | wc -l | tr -d ' ')" = 1 ]
[ -z "$(find "$fixture" -name '.macmon-control.*' -print)" ]
printf 'PASS standalone app resources, quoted paths and non-overwriting installer\n'
