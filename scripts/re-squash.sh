#!/usr/bin/env bash
# The revert-proof re-squash (AGENTS.md, agent-process anti-patterns).
#
# A plain `git reset --soft origin/main` keeps the OLD index: when main
# moved since your last fetch, the fresh commit's tree silently reverts
# every merge that landed in between (observed five times in one week:
# a release version bump and four merged PRs reverted by one PR each).
# This script instead decides everything from TREE OBJECTS first and
# hands git its own patch to stage — the full guarantee map is the
# terminal-outcomes table below.
#
# Usage:
#   scripts/re-squash.sh <source-commit> -F <message-file> <file>...
#
#   <source-commit>  pinned to exactly one commit at capture; its tree
#                    is not trusted — only the named files are read
#                    from it.
#   <file>...        EVERY file your PR intends to change.
#
# Terminal outcomes — the guarantee map, one labeled clause per
# guarantee, then one row per outcome:
#   - every abort before the one final reset leaves the tree untouched
#     BY CONSTRUCTION: the commit object only springs into existence
#     after all gates pass, and nothing before the final reset writes
#     the worktree, the index, or HEAD
#   - every git failure before the final reset — outside the usage
#     rows above — is a branded abort: exit 1, tree untouched
#   - the final reset is the sole mutation that can fail dirty — its
#     row owns the recovery wording
#   usage error / non-commit rev              exit 2
#   dirty worktree                           tree untouched, exit 1
#   unreadable message file                  tree untouched, exit 1
#   HEAD not a local feature branch          tree untouched, exit 1
#   declared path misspelled — a typo
#   (the canonicalization gate classifies)   tree untouched, exit 1
#   main moved a declared path since the
#   source fork point (source predates it)   tree untouched, exit 1
#   declared path in neither source nor main tree untouched, exit 1
#   git failure                                 tree untouched, exit 1
#   untracked file at a declared path        tree untouched, exit 1
#   would-stage path outside the declared list
#                                            tree untouched, exit 1
#   every declared path already matches main tree untouched, exit 0
#   the final reset fails                    tree untouched or
#                                            partially reset — the
#                                            abort message owns the
#                                            recovery step, exit 1
#   success                                  one commit on origin/main

set -euo pipefail

usage() {
    echo "usage: $0 <source-commit> -F <message-file> <file>..." >&2
    exit 2
}

# The one pre-mutation abort: the exit code, the stream, and the
# tree-untouched policy each stated exactly once. The list form prints
# one offending-path list under the same policy line.
abort_line() {
    printf 're-squash: %s — aborting (tree untouched)\n' "$*"
}
die() {
    abort_line "$*" >&2
    exit 1
}
die_list() {
    abort_line "$1" >&2
    printf '%s\n' "$2" >&2
    exit 1
}

# The one option list every origin/main→source diff shares — the gate
# name sets, the staged patch, and the printed verification hint are all
# produced from it, so the validated would-set and the applied patch are
# provably the same diff and the hint cannot drift from the code.
diff_opts=(-c core.quotePath=false diff --no-renames)
diff_names() {
    local a=$1 b=$2
    shift 2
    git "${diff_opts[@]}" --name-only "$a" "$b" -- "$@" \
        | sort
}
# The set operations the gates name, over sorted path lists; an empty
# list is the empty set, not one blank line.
set_lines() { [ -z "$1" ] || printf '%s\n' "$1"; }
only_in_first() { comm -23 <(set_lines "$1") <(set_lines "$2"); }
only_in_second() { comm -13 <(set_lines "$1") <(set_lines "$2"); }
in_both() { comm -12 <(set_lines "$1") <(set_lines "$2"); }

# >= 4: commit, -F, message file, and at least one file.
[ $# -ge 4 ] || usage
src=$(git rev-parse --verify "$1^{commit}") || usage
shift
[ "$1" = "-F" ] || usage
msgfile="$2"
shift 2

# Pin the message file before the cd below re-bases a relative path.
case $msgfile in
    /*) ;;
    *) msgfile="$PWD/$msgfile" ;;
esac

# Pin the declared paths the same way the message file is pinned:
# arguments are cwd-relative pathspecs (git's own rule), canonicalized
# to the repo-root spelling once — the invocation directory's own
# prefix (git rev-parse --show-prefix) is prepended, so the set gates
# below compare one spelling. An absolute path, the empty spelling, or
# a duplicate is a typo, aborted loudly (the gates compare spellings,
# so a mismatch would misreport as a foreign path or as satisfied).
prefix=$(git rev-parse --show-prefix) || die "git rev-parse --show-prefix failed"
paths=()
for f in "$@"; do
    while [ "${f#./}" != "$f" ]; do f=${f#./}; done
    case $f in
        /*) die "declared path $f must be repo-relative, not absolute" ;;
        */) die "declared path $f is not a normalized spelling — a directory form" ;;
        *[[:cntrl:]]*) die "declared path $f contains a control character — this tool takes boring paths only" ;;
    esac
    # Positive canonicalization: every interior path segment must be a
    # real name (not ., .., or empty — an interior // makes an empty
    # segment; a trailing / is the case block's directory arm above).
    seg=$f
    while [ -n "$seg" ]; do
        part=${seg%%/*}
        [ -n "$part" ] && [ "$part" != "." ] && [ "$part" != ".." ] \
            || die "declared path $f is not a normalized spelling — a typo"
        if [ "$seg" = "$part" ]; then seg=""; else seg=${seg#*/}; fi
    done
    [ -n "$f" ] || die "a declared path canonicalizes to the empty string"
    paths+=("$prefix$f")
done
# F-15-6: one declared list, sorted once — the duplicate check and the
# set gates below both consume it.
declared=$(printf '%s\n' "${paths[@]}" | sort)
dup=$(printf '%s\n' "$declared" | uniq -d)
[ -z "$dup" ] || die_list "declared path(s) declared twice" "$dup"
set -- "${paths[@]}"

# git consumes the file arguments as cwd-relative pathspecs while the
# gates compare repo-root-relative lists — run everything from the top.
toplevel=$(git rev-parse --show-toplevel) || die "git rev-parse --show-toplevel failed"
cd "$toplevel"

# Preconditions, all before any mutation: a clean worktree and a
# readable message file.
if ! git diff --quiet || ! git diff --cached --quiet; then
    die "worktree is dirty — commit or stash first"
fi
[ -r "$msgfile" ] || die "message file $msgfile is not readable"

# The final reset rewrites whatever HEAD points at — it must run on a
# local feature branch, never the default branch or a detached HEAD
# (a sibling checkout, a worktree on main: the reset would move that
# ref to the new commit).
head_ref=$(git symbolic-ref -q HEAD) \
    || die "HEAD is detached — re-squash runs on a feature branch only"
# The default branch from origin/HEAD's FULL symref target, converted
# explicitly (a --short read returns the remote-tracking spelling, and
# git writes either refs/heads/… or refs/remotes/origin/… depending on
# how the head was set) — never trust a shortening to be a local name.
default_head=$(git symbolic-ref -q refs/remotes/origin/HEAD) \
    || die "refs/remotes/origin/HEAD is unset — cannot determine the default branch (run: git remote set-head origin -a) and rerun"
case $default_head in
    refs/remotes/origin/*) default_head=${default_head#refs/remotes/origin/} ;;
    refs/heads/*) default_head=${default_head#refs/heads/} ;;
    *) die "origin/HEAD points at unrecognized ref $default_head — refusing to guess the default branch" ;;
esac
[ "$default_head" = main ] \
    || die "origin's default branch is $default_head — this tool re-squashes onto origin/main, refusing rather than half-speaking $default_head"
[ "$head_ref" != "refs/heads/$default_head" ] \
    || die "HEAD is on $default_head — re-squash runs on a feature branch only"

git fetch origin main || die "git fetch origin main failed"
main=$(git rev-parse origin/main) || die "resolving refs/remotes/origin/main failed"

# Untracked files at declared paths — ignored or not, they would be
# overwritten by the final reset, and the tracked-diff guard cannot
# see them (an ignore-rule exclusion here would let the reset clobber
# a gitignored file silently).
untracked=$(git ls-files --others -- "$@") \
    || die "git ls-files failed while scanning for untracked declared paths"
[ -z "$untracked" ] || die_list "untracked file(s) at declared paths would be overwritten" "$untracked"

# The tree gate, before any mutation: what restoring the declared
# paths from the source onto main would stage, computed from tree
# objects alone.
#   a would-stage path outside the declared list — a foreign file or
#       a directory argument's expansion — aborts with the tree
#       untouched
#   a declared path not in the would-stage set — classified below the
#       gate: a typo (aborts) or already matching main (satisfied,
#       reported, excluded)
would=$(diff_names "$main" "$src" "$@") \
    || die "git diff failed while computing the would-stage set"
foreign=$(only_in_first "$would" "$declared")
satisfied=$(only_in_second "$would" "$declared")
[ -z "$foreign" ] || die_list "would-stage paths outside the declared list (foreign file or directory expansion)" "$foreign"
# A declared path not in the would-stage set is either a typo (in
# neither tree — a member present in main but absent from the source
# would be a would-deletion, so only an absent-everywhere path reaches
# the probe) or a file whose source content already matches main —
# classified here, once, over lines.
while IFS= read -r f; do
    git cat-file -e "$src:$f" 2>/dev/null \
        || die "declared file $f is in neither the source commit nor origin/main"
done < <(set_lines "$satisfied")
[ -z "$satisfied" ] || printf 're-squash: already satisfied (matches main, not committed): %s\n' "$satisfied" >&2

# The revert gate the path-set gate cannot express: a declared path
# that main changed since the source's fork point AND that the source
# still differs from main on — restoring the source content there would
# silently revert main's newer change to that same file, the whole-tree
# revert class one path below. Rebase the source first.
base=$(git merge-base "$main" "$src") \
    || die "git merge-base failed — the source commit shares no history with main"
moved=$(diff_names "$base" "$main" "$@") \
    || die "git diff failed while computing the moved-path set"
stale=$(in_both "$moved" "$would")
[ -z "$stale" ] || die_list "main moved declared path(s) since the source commit and the source predates the change — rebase first, a re-squash would revert main" "$stale"

if [ -z "$would" ]; then
    printf 're-squash: every declared file already matches main — nothing to commit (tree untouched)\n' >&2
    exit 0
fi

# The object-space build: a temp index seeded from main's tree, the
# declared paths staged by handing git its own patch (the same
# origin/main→source diff the gates computed, binary form so mode
# changes and deletions ride the one patch), then write-tree +
# commit-tree. Nothing here touches the worktree, the real index, or
# HEAD — a failure is a plain abort, and every blob the patch needs is
# already in the object database.
idx=$(mktemp) || die "mktemp failed while building the object-space commit"
trap 'rm -f "$idx"' EXIT
GIT_INDEX_FILE=$idx git read-tree "$main" \
    || die "git read-tree failed while building the object-space commit"
if ! git "${diff_opts[@]}" --binary "$main" "$src" -- "$@" \
    | GIT_INDEX_FILE=$idx git apply --cached; then
    die "git apply --cached failed while building the object-space commit"
fi
tree=$(GIT_INDEX_FILE=$idx git write-tree) \
    || die "git write-tree failed while building the object-space commit"
new=$(git commit-tree "$tree" -p "$main" -F "$msgfile") \
    || die "git commit-tree failed while building the object-space commit"

# The one worktree mutation. HEAD and the index move with it; a failure
# leaves the branch where it was and is reported with the manual
# recovery step (the commit object already exists, so the rerun after
# the fix lands on the same result).
if ! git reset --hard "$new" >/dev/null; then
    printf 're-squash: the final reset failed — tree untouched or partially reset; fix the cause and rerun, or git reset --hard %s to take the already-built commit\n' "$new" >&2
    exit 1
fi
count=$(printf '%s\n' "$would" | wc -l | tr -d ' ')
printf 're-squash: one commit on %s carrying %s file(s):\n' "$(git rev-parse --short "$new^")" "$count"
printf '%s\n' "$would"
printf 'verify before pushing: git %s --name-only origin/main HEAD must list the same files only\n' "${diff_opts[*]}"
