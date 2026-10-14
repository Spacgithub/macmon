#!/bin/sh
# Merge pinned upstream history into a checked branch in the personal fork.
set -eu

die() { printf 'upstream-sync: %s\n' "$*" >&2; exit 1; }
[ "$#" -eq 0 ] || die 'usage: make upstream-sync'
repo=github.com/Spacgithub/macmon
prefix=feat/upstream-sync-
command -v gh >/dev/null || die 'GitHub CLI is required'
command -v make >/dev/null || die 'Make is required'
local_env=$(git rev-parse --local-env-vars) || die 'cannot inspect Git environment'
for variable in $local_env; do
  eval '[ "${'"$variable"'+set}" != set ]' ||
    die "unset repository-local Git environment variable $variable before syncing"
done
primary=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
[ -n "$primary" ] && [ -d "$primary" ] || die 'cannot locate the primary worktree'
cd "$primary"

fork_url() {
  case "$1" in
    https://github.com/Spacgithub/macmon|https://github.com/Spacgithub/macmon.git|\
    git@github.com:Spacgithub/macmon|git@github.com:Spacgithub/macmon.git|\
    ssh://git@github.com/Spacgithub/macmon|ssh://git@github.com/Spacgithub/macmon.git) ;;
    *) die 'origin must have one effective URL for Spacgithub/macmon' ;;
  esac
}
remotes_ok() {
  fork_url "$(git remote get-url --all origin)"
  fork_url "$(git remote get-url --push --all origin)"
  case "$(git remote get-url --all upstream)" in
    https://github.com/vladkens/macmon|https://github.com/vladkens/macmon.git|\
    git@github.com:vladkens/macmon|git@github.com:vladkens/macmon.git|\
    ssh://git@github.com/vladkens/macmon|ssh://git@github.com/vladkens/macmon.git) ;;
    *) die 'upstream must have one effective URL for vladkens/macmon' ;;
  esac
  [ "$(git remote get-url --push --all upstream)" = /dev/null ] ||
    die 'disable upstream pushes with git remote set-url --push upstream /dev/null'
}
open_pr() {
  gh pr list --repo "$repo" --base main --state open --limit 1000 \
    --json headRefName,url,isCrossRepository \
    --jq '.[] | select(.isCrossRepository == false and (.headRefName | startswith("feat/upstream-sync-"))) | .url'
}
remotes_ok
worktrees="$primary/.worktrees"
[ ! -L "$worktrees" ] || die '.worktrees must not be a symlink'
mkdir -p "$worktrees"
lock="$worktrees/.upstream-sync.lock"
mkdir "$lock" 2>/dev/null || die 'another sync holds .worktrees/.upstream-sync.lock'
body=
trap '[ -z "$body" ] || rm -f "$body"; rmdir "$lock"' 0
trap 'exit 1' HUP INT TERM
previous=$(git rev-parse --verify refs/remotes/upstream/main 2>/dev/null || true)
for remote in origin upstream; do
  if git symbolic-ref -q "refs/remotes/$remote/main" >/dev/null; then
    die 'main tracking refs must not be symbolic'
  fi
done
git fetch --no-tags --no-prune --no-prune-tags --refmap= origin \
  refs/heads/main:refs/remotes/origin/main
fork=$(git rev-parse --verify 'refs/remotes/origin/main^{commit}')
git fetch --no-tags --no-prune --no-prune-tags --refmap= upstream refs/heads/main
upstream=$(git rev-parse --verify 'FETCH_HEAD^{commit}')
if [ -n "$previous" ] && ! git merge-base --is-ancestor "$previous" "$upstream"; then
  die 'upstream history changed non-linearly; review it manually'
fi
git merge-base "$fork" "$upstream" >/dev/null || die 'fork and upstream have unrelated histories'
git symbolic-ref -q refs/remotes/upstream/main >/dev/null && die 'upstream tracking ref became symbolic'
git update-ref --no-deref refs/remotes/upstream/main "$upstream" "$previous"
if git merge-base --is-ancestor "$upstream" "$fork"; then
  printf 'Upstream is already integrated into fork main.\n'
  exit 0
fi
existing=$(open_pr)
if [ -n "$existing" ]; then
  printf '%s\n' "$existing"
  exit 0
fi
fork_short=$(printf '%.12s' "$fork")
upstream_short=$(printf '%.12s' "$upstream")
branch="$prefix$fork_short-$upstream_short"
candidate="$worktrees/upstream-sync-$fork_short-$upstream_short"
commit_needed=true
ready() {
  [ "$(git -C "$candidate" rev-parse --show-toplevel)" = "$candidate" ] &&
    [ "$(git -C "$candidate" symbolic-ref --short HEAD)" = "$branch" ] &&
    [ "$(git -C "$candidate" show -s --format=%P HEAD)" = "$fork $upstream" ] &&
    [ -z "$(git -C "$candidate" status --porcelain --untracked-files=all)" ] || return 1
  for state in MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD rebase-apply rebase-merge; do
    [ ! -e "$(git -C "$candidate" rev-parse --path-format=absolute --git-path "$state")" ] || return 1
  done
}
if [ -e "$candidate" ] || [ -L "$candidate" ]; then
  [ ! -L "$candidate" ] && ready || die "refusing existing candidate: $candidate"
  commit_needed=false
elif git show-ref --verify --quiet "refs/heads/$branch"; then
  [ "$(git show -s --format=%P "$branch")" = "$fork $upstream" ] ||
    die 'existing candidate branch does not match the pinned merge'
  git worktree add "$candidate" "$branch"
  ready || die 'existing branch is not a clean completed sync'
  commit_needed=false
else
  published=$(git ls-remote --heads origin "refs/heads/$branch")
  if [ -n "$published" ]; then
    git fetch --no-tags --no-prune --no-prune-tags --refmap= origin \
      "refs/heads/$branch:refs/remotes/origin/$branch"
    [ "$(git show -s --format=%P "refs/remotes/origin/$branch")" = "$fork $upstream" ] ||
      die 'published candidate does not match the pinned merge'
    git worktree add -b "$branch" --no-track "$candidate" "refs/remotes/origin/$branch"
    ready || die 'published branch is not a clean completed sync'
    commit_needed=false
  else
    git worktree add -b "$branch" --no-track "$candidate" "$fork"
  fi
fi
printf 'Candidate retained at %s\n' "$candidate"
if "$commit_needed"; then
  git -C "$candidate" merge --no-ff --no-commit "$upstream" ||
    die "merge stopped; resolve or inspect the retained candidate: $candidate"
fi
git -C "$candidate" diff --check "$fork"
(cd "$candidate" && remotes_ok && make check && make test)
git -C "$candidate" diff --quiet || die 'checks changed unstaged tracked files'
[ -z "$(git -C "$candidate" ls-files --others --exclude-standard)" ] ||
  die 'checks left untracked files; candidate is retained'
if "$commit_needed"; then
  [ "$(git -C "$candidate" rev-parse HEAD)" = "$fork" ] &&
    [ "$(git -C "$candidate" rev-parse MERGE_HEAD)" = "$upstream" ] ||
    die 'checks changed the pinned merge state'
  git -C "$candidate" commit -m "chore: merge upstream $upstream_short"
fi
ready || die 'candidate is not a clean merge of the pinned commits'
existing=$(open_pr)
if [ -n "$existing" ]; then
  printf '%s\n' "$existing"
  exit 0
fi
(cd "$candidate" && remotes_ok)
git -C "$candidate" -c remote.origin.mirror=false push --no-follow-tags \
  --recurse-submodules=no -u origin "refs/heads/$branch:refs/heads/$branch"
body=$(mktemp "${TMPDIR:-/tmp}/macmon-upstream-sync.XXXXXX")
cat >"$body" <<EOF
Merge upstream main \`$upstream\` into fork main \`$fork\`, preserving both histories and fork features.

Checks: \`git diff --check\`, \`make check\`, \`make test\`.
Review changed workflows and run relevant hardware checks before merging.

**Use a merge commit for this sync PR; do not squash or rebase it.**
EOF
gh pr create --repo "$repo" --base main --head "$branch" \
  --title "chore: merge upstream $upstream_short" --body-file "$body"
