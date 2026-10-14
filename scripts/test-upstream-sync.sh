#!/bin/sh
# Fast, isolated Git fixtures; all network/publication commands are local stubs.
set -eu
scripts=$(CDPATH= cd "$(dirname "$0")" && pwd)
sync_script="$scripts/upstream-sync.sh"
REAL_GIT=$(command -v git)
export REAL_GIT
test_root=$(mktemp -d "${TMPDIR:-/tmp}/macmon-sync-tests.XXXXXX")
trap 'rm -rf "$test_root"' 0
trap 'exit 1' HUP INT TERM
for variable in $("$REAL_GIT" rev-parse --local-env-vars); do unset "$variable"; done
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_ALLOW_PROTOCOL=file GIT_TERMINAL_PROMPT=0
export GIT_AUTHOR_NAME=Fixture GIT_COMMITTER_NAME=Fixture
export GIT_AUTHOR_EMAIL=fixture@example.invalid GIT_COMMITTER_EMAIL=fixture@example.invalid
mkdir "$test_root/bin"
cat >"$test_root/bin/git" <<'EOF'
#!/bin/sh
set -eu
cwd=$(pwd)
if [ "${1:-}" = -C ]; then cwd=$2; shift 2; fi
mirror=false
while [ "${1:-}" = -c ]; do
  [ "$2" = remote.origin.mirror=false ] || exit 90
  mirror=true
  shift 2
done
command=$1
shift
case "$command" in
  fetch)
    tags=false; prune=false; prune_tags=false; refmap=false
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --no-tags) tags=true ;;
        --no-prune) prune=true ;;
        --no-prune-tags) prune_tags=true ;;
        --refmap=) refmap=true ;;
        *) break ;;
      esac
      shift
    done
    "$tags" && "$prune" && "$prune_tags" && "$refmap" || exit 91
    remote=$1; shift
    [ "$#" -eq 1 ] || exit 92
    case "$remote" in origin) source=$FIXTURE_ORIGIN ;; upstream) source=$FIXTURE_UPSTREAM ;; *) exit 93 ;; esac
    printf 'fetch %s %s\n' "$remote" "$1" >>"$FIXTURE_DIR/events"
    exec "$REAL_GIT" -C "$cwd" fetch --no-tags --no-prune --no-prune-tags --refmap= "$source" "$@"
    ;;
  ls-remote)
    [ "$#" -eq 3 ] && [ "$1" = --heads ] && [ "$2" = origin ] || exit 94
    exec "$REAL_GIT" -C "$cwd" ls-remote --heads "$FIXTURE_ORIGIN" "$3"
    ;;
  push)
    "$mirror" || exit 95
    [ "$#" -eq 5 ] && [ "$1" = --no-follow-tags ] && [ "$2" = --recurse-submodules=no ] &&
      [ "$3" = -u ] && [ "$4" = origin ] || exit 96
    refspec=$5
    source=${refspec%:*}; destination=${refspec#*:}
    case "$source" in refs/heads/feat/upstream-sync-*) ;; *) exit 97 ;; esac
    [ "$source" = "$destination" ] || exit 98
    printf 'push %s\n' "$refspec" >>"$FIXTURE_DIR/events"
    [ "${FAIL_PUSH:-0}" != 1 ] || exit 99
    exec "$REAL_GIT" -C "$cwd" -c remote.origin.mirror=false push --no-follow-tags --recurse-submodules=no \
      "$FIXTURE_ORIGIN" "$refspec"
    ;;
  *) exec "$REAL_GIT" -C "$cwd" "$command" "$@" ;;
esac
EOF
cat >"$test_root/bin/gh" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = pr ] || exit 90
action=$2; shift 2
repo=; base=; head=; state=; body=; expression=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo) repo=$2 ;;
    --base) base=$2 ;;
    --head) head=$2 ;;
    --state) state=$2 ;;
    --body-file) body=$2 ;;
    --jq) expression=$2 ;;
    --limit|--json|--title) ;;
    *) exit 91 ;;
  esac
  shift 2
done
[ "$repo" = Spacgithub/macmon ] && [ "$base" = main ] || exit 92
case "$action" in
  list)
    [ "$state" = open ] || exit 93
    case "$expression" in *'isCrossRepository == false'*'feat/upstream-sync-'*) ;; *) exit 94 ;; esac
    printf 'pr-list\n' >>"$FIXTURE_DIR/events"
    [ ! -f "$FIXTURE_DIR/open-pr" ] || cat "$FIXTURE_DIR/open-pr"
    ;;
  create)
    case "$head" in feat/upstream-sync-*) ;; *) exit 95 ;; esac
    [ -f "$body" ] || exit 96
    case "$(cat "$body")" in *'Use a merge commit'*'do not squash or rebase'*) ;; *) exit 97 ;; esac
    printf 'pr-create %s\n' "$head" >>"$FIXTURE_DIR/events"
    [ "${FAIL_PR:-0}" != 1 ] || exit 98
    printf 'https://github.com/Spacgithub/macmon/pull/99\n' >"$FIXTURE_DIR/open-pr"
    cat "$FIXTURE_DIR/open-pr"
    ;;
  *) exit 99 ;;
esac
EOF
chmod +x "$test_root/bin/git" "$test_root/bin/gh"
PATH="$test_root/bin:$PATH"
export PATH
die() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
fixture() {
  FIXTURE_DIR=$(mktemp -d "$test_root/case.XXXXXX")
  FIXTURE_ORIGIN="$FIXTURE_DIR/fork.git"
  FIXTURE_UPSTREAM="$FIXTURE_DIR/upstream.git"
  export FIXTURE_DIR FIXTURE_ORIGIN FIXTURE_UPSTREAM
  FAIL_CHECK=0; FAIL_TEST=0; FAIL_PUSH=0; FAIL_PR=0
  export FAIL_CHECK FAIL_TEST FAIL_PUSH FAIL_PR
  "$REAL_GIT" init -q -b main "$FIXTURE_DIR/author"
  printf '.worktrees/\n' >"$FIXTURE_DIR/author/.gitignore"
  printf 'base\n' >"$FIXTURE_DIR/author/shared.txt"
  printf 'check:\n\t@test "$$FAIL_CHECK" != 1\n\t@test -f personal-network.marker\ntest:\n\t@test "$$FAIL_TEST" != 1\n\t@test -f upstream-improvement.marker\n' >"$FIXTURE_DIR/author/Makefile"
  "$REAL_GIT" -C "$FIXTURE_DIR/author" add .
  "$REAL_GIT" -C "$FIXTURE_DIR/author" commit -qm base
  base=$($REAL_GIT -C "$FIXTURE_DIR/author" rev-parse HEAD)
  "$REAL_GIT" clone -q --bare "$FIXTURE_DIR/author" "$FIXTURE_ORIGIN"
  "$REAL_GIT" clone -q --bare "$FIXTURE_DIR/author" "$FIXTURE_UPSTREAM"
  "$REAL_GIT" clone -q "$FIXTURE_ORIGIN" "$FIXTURE_DIR/personal checkout"
  primary="$FIXTURE_DIR/personal checkout"
  printf 'network feature\n' >"$primary/personal-network.marker"
  "$REAL_GIT" -C "$primary" add .
  "$REAL_GIT" -C "$primary" commit -qm 'personal feature'
  "$REAL_GIT" -C "$primary" push -q origin main
  fork=$($REAL_GIT -C "$primary" rev-parse HEAD)
  printf 'upstream improvement\n' >"$FIXTURE_DIR/author/upstream-improvement.marker"
  "$REAL_GIT" -C "$FIXTURE_DIR/author" add .
  "$REAL_GIT" -C "$FIXTURE_DIR/author" commit -qm improvement
  "$REAL_GIT" -C "$FIXTURE_DIR/author" push -q "$FIXTURE_UPSTREAM" main
  upstream=$($REAL_GIT -C "$FIXTURE_DIR/author" rev-parse HEAD)
  "$REAL_GIT" -C "$primary" remote set-url origin git@github.com:Spacgithub/macmon.git
  "$REAL_GIT" -C "$primary" remote add upstream https://github.com/vladkens/macmon.git
  "$REAL_GIT" -C "$primary" remote set-url --push upstream /dev/null
  branch="feat/upstream-sync-$(printf '%.12s' "$fork")-$(printf '%.12s' "$upstream")"
  candidate="$primary/.worktrees/upstream-sync-$(printf '%.12s' "$fork")-$(printf '%.12s' "$upstream")"
  : >"$FIXTURE_DIR/events"
}
run_sync() { (cd "$primary" && sh "$sync_script") >"$FIXTURE_DIR/output" 2>&1; }
success() { run_sync || { cat "$FIXTURE_DIR/output" >&2; die "$*"; }; }
failure() { if run_sync; then die "unexpected success: $*"; fi; }
unpublished() {
  if grep -E '^push |^pr-create ' "$FIXTURE_DIR/events" >/dev/null; then die 'published an unsafe candidate'; fi
}
unchanged_main() {
  [ "$($REAL_GIT --git-dir="$FIXTURE_ORIGIN" rev-parse refs/heads/main)" = "$fork" ] || die 'fork main changed'
  [ "$($REAL_GIT -C "$primary" rev-parse HEAD)" = "$fork" ] || die 'primary HEAD changed'
}

fixture
printf 'dirty user change\n' >>"$primary/shared.txt"
printf 'staged user change\n' >>"$primary/personal-network.marker"
"$REAL_GIT" -C "$primary" add personal-network.marker
printf 'untracked user file\n' >"$primary/user.tmp"
status_before=$($REAL_GIT -C "$primary" status --porcelain)
diff_before=$($REAL_GIT -C "$primary" diff HEAD)
success 'preserve fork features and user edits'
[ -f "$candidate/personal-network.marker" ] && [ -f "$candidate/upstream-improvement.marker" ] || die 'lost either feature'
[ "$(cat "$candidate/personal-network.marker")" = 'network feature' ] || die 'imported dirty user changes'
[ "$($REAL_GIT -C "$candidate" show -s --format=%P HEAD)" = "$fork $upstream" ] || die 'did not preserve ancestry'
[ "$($REAL_GIT -C "$primary" status --porcelain)" = "$status_before" ] || die 'dirty status changed'
[ "$($REAL_GIT -C "$primary" diff HEAD)" = "$diff_before" ] || die 'dirty diff changed'
[ "$(cat "$primary/user.tmp")" = 'untracked user file' ] || die 'untracked user file changed'
unchanged_main
count=$(grep -c '^pr-create ' "$FIXTURE_DIR/events")
success 'existing open PR'
[ "$(grep -c '^pr-create ' "$FIXTURE_DIR/events")" = "$count" ] || die 'duplicated open PR'
printf 'ok - preservation, isolation, dirty checkout, existing PR\n'

# Simulate the person merging the sync PR with a merge commit, then a later upstream change.
first=$($REAL_GIT -C "$candidate" rev-parse HEAD)
"$REAL_GIT" --git-dir="$FIXTURE_ORIGIN" update-ref refs/heads/main "$first"
rm "$FIXTURE_DIR/open-pr"
success 'repeat after merge'
grep 'already integrated' "$FIXTURE_DIR/output" >/dev/null || die 'integrated upstream was not a no-op'
printf 'second improvement\n' >"$FIXTURE_DIR/author/later.marker"
"$REAL_GIT" -C "$FIXTURE_DIR/author" add .
"$REAL_GIT" -C "$FIXTURE_DIR/author" commit -qm 'later improvement'
"$REAL_GIT" -C "$FIXTURE_DIR/author" push -q "$FIXTURE_UPSTREAM" main
later=$($REAL_GIT -C "$FIXTURE_DIR/author" rev-parse HEAD)
success 'successive sync'
second_branch="feat/upstream-sync-$(printf '%.12s' "$first")-$(printf '%.12s' "$later")"
second="$primary/.worktrees/upstream-sync-$(printf '%.12s' "$first")-$(printf '%.12s' "$later")"
[ -f "$second/personal-network.marker" ] && [ -f "$second/upstream-improvement.marker" ] && [ -f "$second/later.marker" ] || die 'successive sync lost features'
"$REAL_GIT" -C "$second" merge-base --is-ancestor "$first" HEAD || die 'lost previous sync ancestry'
[ "$(grep -c '^pr-create ' "$FIXTURE_DIR/events")" -eq 2 ] || die 'wrong successive PR count'
printf 'ok - repeated and successive sync ancestry\n'

fixture
printf 'personal conflict\n' >"$primary/shared.txt"
"$REAL_GIT" -C "$primary" add .
"$REAL_GIT" -C "$primary" commit -qm 'personal edit'
fork=$($REAL_GIT -C "$primary" rev-parse HEAD)
"$REAL_GIT" -C "$primary" push -q "$FIXTURE_ORIGIN" main
printf 'upstream conflict\n' >"$FIXTURE_DIR/author/shared.txt"
"$REAL_GIT" -C "$FIXTURE_DIR/author" add .
"$REAL_GIT" -C "$FIXTURE_DIR/author" commit -qm 'upstream edit'
upstream=$($REAL_GIT -C "$FIXTURE_DIR/author" rev-parse HEAD)
"$REAL_GIT" -C "$FIXTURE_DIR/author" push -q "$FIXTURE_UPSTREAM" main
candidate="$primary/.worktrees/upstream-sync-$(printf '%.12s' "$fork")-$(printf '%.12s' "$upstream")"
failure conflict
unpublished
"$REAL_GIT" -C "$candidate" rev-parse --verify MERGE_HEAD >/dev/null || die 'conflict candidate was lost'
failure 'rerun of conflicted candidate'
unpublished
unchanged_main
printf 'ok - conflicts preserved without publishing\n'

for check in FAIL_CHECK FAIL_TEST; do
  fixture
  case "$check" in FAIL_CHECK) FAIL_CHECK=1 ;; FAIL_TEST) FAIL_TEST=1 ;; esac
  failure "$check"
  unpublished
  "$REAL_GIT" -C "$candidate" rev-parse --verify MERGE_HEAD >/dev/null || die 'failed-check candidate was lost'
  unchanged_main
done
printf 'ok - failed checks prevent publication\n'

for invalid in fetch-url push-url extra-push-url duplicate-push-url fetch-rewrite push-rewrite upstream-url upstream-push symbolic-ref; do
  fixture
  case "$invalid" in
    fetch-url) "$REAL_GIT" -C "$primary" remote set-url origin git@github.com:vladkens/macmon.git ;;
    push-url) "$REAL_GIT" -C "$primary" remote set-url --push origin git@github.com:vladkens/macmon.git ;;
    extra-push-url) "$REAL_GIT" -C "$primary" config --add remote.origin.pushurl git@github.com:Spacgithub/macmon.git; "$REAL_GIT" -C "$primary" config --add remote.origin.pushurl git@github.com:vladkens/macmon.git ;;
    duplicate-push-url) "$REAL_GIT" -C "$primary" config --add remote.origin.pushurl git@github.com:Spacgithub/macmon.git; "$REAL_GIT" -C "$primary" config --add remote.origin.pushurl git@github.com:Spacgithub/macmon.git ;;
    fetch-rewrite) "$REAL_GIT" -C "$primary" config url.https://github.com/vladkens/macmon.git.insteadOf git@github.com:Spacgithub/macmon.git ;;
    push-rewrite) "$REAL_GIT" -C "$primary" config url.https://github.com/vladkens/macmon.git.pushInsteadOf git@github.com:Spacgithub/macmon.git ;;
    upstream-url) "$REAL_GIT" -C "$primary" remote set-url upstream https://github.com/other/macmon.git ;;
    upstream-push) "$REAL_GIT" -C "$primary" remote set-url --push upstream https://github.com/vladkens/macmon.git ;;
    symbolic-ref) "$REAL_GIT" -C "$primary" symbolic-ref refs/remotes/origin/main refs/heads/main ;;
  esac
  failure "$invalid"
  unpublished
  unchanged_main
done
printf 'ok - effective URLs, rewrites, extra push URLs, tracking refs\n'

fixture
mkdir -p "$candidate"
printf 'existing user candidate\n' >"$candidate/user.txt"
failure 'existing non-worktree candidate'
[ "$(cat "$candidate/user.txt")" = 'existing user candidate' ] || die 'overwrote existing candidate'
unpublished
printf 'ok - existing candidate is not overwritten\n'

for partial in FAIL_PUSH FAIL_PR; do
  fixture
  case "$partial" in FAIL_PUSH) FAIL_PUSH=1 ;; FAIL_PR) FAIL_PR=1 ;; esac
  failure "$partial"
  before=$($REAL_GIT -C "$candidate" rev-parse HEAD)
  FAIL_PUSH=0; FAIL_PR=0
  success 'resume committed candidate'
  [ "$($REAL_GIT -C "$candidate" rev-parse HEAD)" = "$before" ] || die 'recommitted existing merge'
  count=$(grep -c '^pr-create ' "$FIXTURE_DIR/events")
  success 'rerun after resumed publication'
  [ "$(grep -c '^pr-create ' "$FIXTURE_DIR/events")" = "$count" ] || die 'duplicated resumed PR'
done
printf 'ok - partial push/PR failures recover without duplicate PRs\n'

fixture
FAIL_PR=1
failure 'prepare pending-merge fixture'
FAIL_PR=0
merge_state=$($REAL_GIT -C "$candidate" rev-parse --path-format=absolute --git-path MERGE_HEAD)
printf '%s\n' "$upstream" >"$merge_state"
: >"$FIXTURE_DIR/events"
[ -z "$($REAL_GIT -C "$candidate" status --porcelain)" ] || die 'pending empty merge fixture is not clean'
failure 'pending empty merge'
[ -f "$merge_state" ] || die 'deleted pending merge state'
unpublished
printf 'ok - pending empty merge is refused\n'

fixture
FAIL_PR=1
failure 'prepare dirty completed candidate'
FAIL_PR=0
printf 'candidate user change\n' >>"$candidate/shared.txt"
: >"$FIXTURE_DIR/events"
failure 'dirty completed candidate'
grep 'candidate user change' "$candidate/shared.txt" >/dev/null || die 'overwrote dirty candidate'
unpublished
printf 'ok - dirty completed candidate is refused\n'

for invalid in push-url extra-push-url; do
  fixture
  FAIL_PR=1
  failure 'prepare worktree-specific push configuration'
  FAIL_PR=0
  "$REAL_GIT" -C "$primary" config extensions.worktreeConfig true
  if [ "$invalid" = extra-push-url ]; then
    "$REAL_GIT" -C "$candidate" config --worktree --add remote.origin.pushurl git@github.com:Spacgithub/macmon.git
  fi
  "$REAL_GIT" -C "$candidate" config --worktree --add remote.origin.pushurl git@github.com:vladkens/macmon.git
  printf 'dirty primary\n' >>"$primary/shared.txt"
  status_before=$("$REAL_GIT" -C "$primary" status --porcelain)
  diff_before=$("$REAL_GIT" -C "$primary" diff HEAD)
  index_before=$(cksum "$primary/.git/index")
  : >"$FIXTURE_DIR/events"
  failure "candidate-specific $invalid"
  unpublished
  [ "$(cksum "$primary/.git/index")" = "$index_before" ] || die 'candidate configuration changed primary index'
  [ "$("$REAL_GIT" -C "$primary" status --porcelain)" = "$status_before" ] || die 'candidate configuration changed dirty status'
  [ "$("$REAL_GIT" -C "$primary" diff HEAD)" = "$diff_before" ] || die 'candidate configuration changed dirty diff'
  unchanged_main
done
printf 'ok - candidate effective push URL is checked before publication\n'

fixture
FAIL_PR=1
failure 'prepare remote-only recovery'
FAIL_PR=0
before=$($REAL_GIT -C "$candidate" rev-parse HEAD)
"$REAL_GIT" -C "$primary" worktree remove "$candidate"
"$REAL_GIT" -C "$primary" update-ref -d "refs/heads/$branch"
success 'recover published branch without local candidate'
[ "$($REAL_GIT -C "$candidate" rev-parse HEAD)" = "$before" ] || die 'remote-only recovery replaced the published commit'
printf 'ok - published branch recovery\n'

fixture
"$REAL_GIT" -C "$primary" fetch -q --no-tags "$FIXTURE_UPSTREAM" refs/heads/main:refs/remotes/upstream/main
"$REAL_GIT" --git-dir="$FIXTURE_UPSTREAM" update-ref refs/heads/main "$base"
failure 'rewritten upstream'
unpublished
[ "$("$REAL_GIT" -C "$primary" rev-parse refs/remotes/upstream/main)" = "$upstream" ] || die 'rewrite changed tracking baseline'
failure 'repeated rewritten upstream'
unpublished
[ "$("$REAL_GIT" -C "$primary" rev-parse refs/remotes/upstream/main)" = "$upstream" ] || die 'repeat changed tracking baseline'
fixture
"$REAL_GIT" init -q -b main "$FIXTURE_DIR/unrelated"
printf 'unrelated\n' >"$FIXTURE_DIR/unrelated/other.txt"
"$REAL_GIT" -C "$FIXTURE_DIR/unrelated" add .
"$REAL_GIT" -C "$FIXTURE_DIR/unrelated" commit -qm unrelated
unrelated=$($REAL_GIT -C "$FIXTURE_DIR/unrelated" rev-parse HEAD)
"$REAL_GIT" -C "$primary" fetch -q "$FIXTURE_DIR/unrelated" main
"$REAL_GIT" --git-dir="$FIXTURE_UPSTREAM" fetch -q "$FIXTURE_DIR/unrelated" main
"$REAL_GIT" --git-dir="$FIXTURE_UPSTREAM" update-ref refs/heads/main "$unrelated"
failure 'unrelated upstream'
unpublished
"$REAL_GIT" -C "$primary" show-ref --verify --quiet refs/remotes/upstream/main && die 'accepted unrelated tracking baseline'
printf 'ok - rewritten and unrelated history requires manual review\n'

for variable in $("$REAL_GIT" rev-parse --local-env-vars); do
  fixture
  printf 'dirty user change\n' >>"$primary/shared.txt"
  printf 'staged user change\n' >>"$primary/personal-network.marker"
  "$REAL_GIT" -C "$primary" add personal-network.marker
  status_before=$("$REAL_GIT" -C "$primary" status --porcelain)
  diff_before=$("$REAL_GIT" -C "$primary" diff HEAD)
  index_before=$(cksum "$primary/.git/index")
  # Empty counts as set; values and filenames must never be printed by the guard.
  if (export "$variable="; run_sync); then die "accepted $variable"; fi
  [ ! -s "$FIXTURE_DIR/events" ] || die "$variable allowed fetch or publication"
  [ "$(cksum "$primary/.git/index")" = "$index_before" ] || die "$variable changed the primary index"
  [ "$("$REAL_GIT" -C "$primary" status --porcelain)" = "$status_before" ] || die "$variable changed dirty status"
  [ "$("$REAL_GIT" -C "$primary" diff HEAD)" = "$diff_before" ] || die "$variable changed dirty diff"
done
fixture
index_before=$(cksum "$primary/.git/index")
if (export GIT_INDEX_FILE="$primary/.git/index"; run_sync); then die 'accepted inherited primary index'; fi
[ ! -s "$FIXTURE_DIR/events" ] || die 'inherited primary index allowed fetch or publication'
[ "$(cksum "$primary/.git/index")" = "$index_before" ] || die 'changed inherited primary index'
printf 'ok - repository-local Git environment is refused before repo access\n'
printf 'All upstream-sync fixtures passed.\n'
