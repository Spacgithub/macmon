#!/bin/sh
# Frozen controller from main before PR #18 (9cac4e5); keep independent of the current controller.
# Session-only desktop control; never changes login agents or Disabled preferences.
set -eu
umask 077
fail() { printf '%s\n' "macmon control: $*" >&2; exit 1; }
action=${1:-}; binary=${2:-}
case "$action" in start|open|status|stop|install) ;; *) fail 'Use start|open|status|stop BINARY, or install BINARY DEST.app';; esac
case "$binary" in /*) ;; *) fail 'The executable must have an absolute path';; esac
case "$binary" in *'
'*) fail 'Newlines in paths are unsupported';; esac
case "$action" in install|start|open)
  [ -f "$binary" ] && [ -x "$binary" ] || fail 'Executable not found'
  version=$("$binary" --version) || fail 'Executable version check failed'
  case "$version" in 'macmon '*) ;; *) fail 'Expected a macmon executable';; esac
esac

if [ "$action" = install ]; then
  destination=${3:-}
  case "$destination" in /*.app) ;; *) fail 'Use an absolute .app destination';; esac
  [ ! -e "$destination" ] && [ ! -L "$destination" ] || fail 'Destination already exists'
  source_dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd -P)
  parent=$(dirname "$destination")
  stage=$(mktemp -d "$parent/.macmon-control.XXXXXX")
  staged_app="$stage/$(basename "$destination")"
  trap 'rm -rf "$stage"' EXIT
  trap 'exit 130' HUP INT TERM
  osacompile -o "$staged_app" "$source_dir/dashboard-control.applescript"
  resources="$staged_app/Contents/Resources"
  cp "$source_dir/dashboard-control.sh" "$resources/control.sh"
  chmod 755 "$resources/control.sh"
  printf '%s\n' "$binary" > "$resources/binary.txt"
  plutil -replace CFBundleName -string 'macmon Web Control' "$staged_app/Contents/Info.plist"
  plutil -replace CFBundleIdentifier -string com.macmon.web-control "$staged_app/Contents/Info.plist"
  plutil -replace LSUIElement -bool true "$staged_app/Contents/Info.plist"
  codesign --force --sign - "$staged_app"
  codesign --verify --deep --strict "$staged_app"
  mv -n "$staged_app" "$parent/"
  [ ! -e "$staged_app" ] || fail 'Destination appeared during installation'
  printf 'Installed %s\n' "$destination"
  exit
fi

uid=$(id -u); domain="gui/$uid"; label=com.macmon; target="$domain/$label"
state=${MACMON_CONTROL_STATE_DIR:-"$HOME/Library/Application Support/macmon/dashboard-control"}
agent=${MACMON_CONTROL_AGENT_PLIST:-"$HOME/Library/LaunchAgents/com.macmon.plist"}
plist="$state/com.macmon.plist"; host=127.0.0.1; port=19090
[ ! -e "$agent" ] && [ ! -L "$agent" ] || plist=$agent
value() { plutil -extract "$1" raw -o - "$plist"; }
if [ -e "$plist" ] || [ -L "$plist" ]; then
  [ -f "$plist" ] && [ ! -L "$plist" ] && [ "$(stat -f %u "$plist")" = "$uid" ] || fail 'Unowned agent plist'
  [ "$(value Label)" = "$label" ] && [ "$(value ProgramArguments)" = 6 ] || fail 'Incompatible agent plist'
  host=$(value ProgramArguments.3); port=$(value ProgramArguments.5)
  [ "$(value ProgramArguments.0)" = "$binary" ] && [ "$(value ProgramArguments.1)" = serve ] &&
    [ "$(value ProgramArguments.2)" = --host ] && [ "$(value ProgramArguments.4)" = --port ] || fail 'Foreign agent executable or arguments'
  if program_type=$(plutil -type Program "$plist" 2>/dev/null); then
    [ "$program_type" = string ] && [ "$(value Program)" = "$binary" ] || fail 'Foreign agent Program override'
  fi
fi
case "$host" in 127.0.0.1|0.0.0.0) ;; *) fail 'Agent host must be 127.0.0.1 or 0.0.0.0';; esac
case "$port" in ''|*[!0-9]*) fail 'Invalid agent port';; esac
[ "$port" -ge 1 ] && [ "$port" -le 65535 ] || fail 'Invalid agent port'
url="http://127.0.0.1:$port/dashboard/"
field() { printf '%s\n' "$info" | sed -n "s/^[[:space:]]*$1 = //p"; }
job() {
  if info=$(launchctl print "$target" 2>&1); then :
  else
    result=$?
    if [ "$result" = 113 ] && [ "$info" = "$(printf 'Bad request.\nCould not find service "%s" in domain for user gui: %s' "$label" "$uid")" ]; then return 1; fi
    fail "Unable to query $target; job may still be loaded: $info"
  fi
  args=$(printf '%s\n' "$info" | awk '/^[[:space:]]*arguments = \{$/ {inside=1;next} inside && /^[[:space:]]*\}$/ {exit} inside {sub(/^[[:space:]]*/, "");print}')
  [ "$(field program)" = "$binary" ] && [ "$(field path)" = "$plist" ] &&
    [ "$args" = "$(printf '%s\n' "$binary" serve --host "$host" --port "$port")" ] || fail 'Foreign loaded job; refusing to control it'
  pid=$(field pid)
}
listener() { lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || :; }
loaded=no; pid=
if job; then loaded=yes; fi
listening=$(listener)
[ -z "$listening" ] || { [ -n "$pid" ] && [ "$listening" = "$pid" ]; } || fail "Port $port belongs to another process"
if [ "$action" = status ]; then
  if [ "$loaded" = no ]; then printf 'Stopped — %s\n' "$url"
  elif [ -z "$listening" ]; then printf 'Loaded, not listening — %s\n' "$url"
  else printf 'Running — %s\n' "$url"; fi
  exit
fi
if [ "$action" = stop ]; then
  if [ "$loaded" = yes ]; then job && launchctl bootout "$target"; fi
  n=0
  while job || [ -n "$(listener)" ]; do
    n=$((n + 1)); [ "$n" -lt 30 ] || fail 'Job or listener did not stop'
    sleep 0.1
  done
  printf 'Stopped — %s\n' "$url"
  exit
fi

started=no; ready=no; building=
cleanup() {
  [ -z "$building" ] || rm -rf "$building"
  if [ "$started" = yes ] && [ "$ready" = no ] && job; then
    launchctl bootout "$target" >/dev/null 2>&1 || { printf 'macmon control: Startup cleanup failed for %s\n' "$target" >&2; return 1; }
    if job; then printf 'macmon control: Job remains loaded after cleanup\n' >&2; return 1; fi
  fi
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
if [ "$loaded" = no ]; then
  if [ ! -e "$plist" ]; then
    [ ! -L "$state" ] || fail 'Unsafe control state directory'
    (umask 077; mkdir -p "$state")
    [ "$(stat -f %u "$state")" = "$uid" ] && [ "$(stat -f %Lp "$state")" = 700 ] || fail 'Unsafe control state directory'
    building=$(mktemp -d "$state/.com.macmon.XXXXXX")
    pending="$building/com.macmon.plist"
    plutil -create xml1 "$pending"
    plutil -insert Label -string "$label" "$pending"
    plutil -insert ProgramArguments -array "$pending"
    index=0
    for arg in "$binary" serve --host "$host" --port "$port"; do
      plutil -insert "ProgramArguments.$index" -string "$arg" "$pending"
      index=$((index + 1))
    done
    plutil -insert KeepAlive -bool true "$pending"
    plutil -insert RunAtLoad -bool true "$pending"
    mv -n "$pending" "$state/"
    [ ! -e "$pending" ] || fail 'Agent plist appeared during creation'
    rmdir "$building"
    building=
  fi
  launchctl bootstrap "$domain" "$plist"
  started=yes
elif [ -z "$listening" ]; then
  job || fail 'Job disappeared before activation'
  launchctl kickstart -k "$target"
  started=yes
fi
n=0
while [ "$n" -lt 30 ]; do
  if job && [ -n "$pid" ] && [ "$(listener)" = "$pid" ] && curl -q --noproxy '*' -fsS --max-time 1 "$url" >/dev/null; then
    ready=yes
    printf 'Running — %s\n' "$url"
    [ "$action" != open ] || open "$url"
    exit
  fi
  n=$((n + 1)); sleep 0.2
done
fail 'Service did not become available'
