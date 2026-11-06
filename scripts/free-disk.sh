#!/usr/bin/env bash
# Free runner disk fast (LABS-310).
#
# The old serial `rm -rf` of the preinstalled toolchains spent ~2:20 per
# runner in a per-file unlink storm: tens of thousands of small files, one
# syscall each. The cleanup is therefore one escalating flow:
#
#   0. Skip (conditional): with MIN_FREE_MB or more already free, do nothing —
#      the tree scan alone (a readdir/stat storm over the toolchain trees on
#      a slow runner disk) costs over a minute. The threshold must exceed
#      what a FRESH free-tier runner shows: ubuntu-latest starts with only
#      ~12-13.5 GB free, and the heavy lanes (feature-bundles builds the
#      workspace three ways plus doctests) peak at ~12 GB — run 38758089160
#      skipped cleanup at 12138 MB free (over the old 10240 default) and
#      ENOSPC'd in the doctests link at 61 MB free. The default therefore
#      sits above any fresh-runner reading so free-tier lanes always clean;
#      the per-dir goal check still stops the deletes as soon as they reach
#      it.
#   1. Parallel full delete: remove the trees one dir at a time, one
#      top-level subdir per core within the dir — the unlink storm amortized
#      over nproc — re-checking the free-space goal after each dir and
#      stopping as soon as it is met.
#   2. Docker prune (conditional): only if still below MIN_FREE_MB after
#      the deletes. It walks the whole image store serially, and on a fresh
#      runner it frees nothing until the job has pulled anything.
#
# There is deliberately no "delete only the LARGE files first" pass. It was
# measured pure waste on the current runner images (run 38780496951): a
# fresh free-tier runner shows ~12.7 GB free and the four default trees
# hold ~11 GB total, so reaching the 20 GB goal requires deleting nearly
# all of them — the large-only pass never met the goal on any of the six
# measured lanes (the mode was always large+parallel-full), and its serial
# per-file `find -size +50M -delete` stat storm cost 27-230 s per lane
# before the parallel full delete (20-40 s) ran anyway. The "few hundred
# unlinks free almost all the space" premise is wrong on these runners: the
# scan itself, not the deletes, is the storm.
#
# The mode that ran ("skipped" included) goes to $GITHUB_STEP_SUMMARY so we
# learn whether the delete pass alone suffices on each runner generation
# (the "leave the cleanup off" experiment, made automatic: later phases are
# skipped unless space demands them).
#
# Modes (which trees count as "safe to delete"):
#
#   default          the 4-dir set (dotnet, android, ghc, CodeQL) — jobs that
#                    later run setup-java / setup-node / setup-go need the
#                    system JVM and node_modules intact, and the language
#                    client lanes preinstall nothing else worth the space.
#   FREE_DISK_AGGRESSIVE=1
#                    the full 8-dir set, additionally trimming the Swift,
#                    JVM, node_modules and boost trees. Only for pure-Rust
#                    jobs that run no language setup action afterwards (the
#                    feature-bundles lane, whose three feature builds plus
#                    doctests outgrew the 4-dir trim — ENOSPC at link time
#                    in run 38758089160).
#
# Wire the mode per call site to what the site deleted BEFORE the
# consolidation (the big list only ever ran in the heaviest pure-Rust job).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# free_mb + step_summary_append (shared helpers; do not paste copies here).
# shellcheck source=lib/common.sh
source "$REPO/scripts/lib/common.sh"

MIN_FREE_MB="${MIN_FREE_MB:-20480}"
# Largest trees first: the pass loop stops at the free-space goal, so the
# biggest trees going first means the fewest dir scans before we exit.
DIRS=(
  /usr/local/lib/android
  /usr/share/dotnet
  /opt/ghc
  /opt/hostedtoolcache/CodeQL
)
if [ "${FREE_DISK_AGGRESSIVE:-0}" = "1" ]; then
  DIRS+=(
    /usr/share/swift
    /usr/lib/jvm
    /usr/local/lib/node_modules
    /usr/local/share/boost
  )
fi

# The one free-space goal every phase escalates toward — the only place the
# free_mb-vs-MIN_FREE_MB predicate lives.
goal_met() {
  [ "$(free_mb)" -ge "$MIN_FREE_MB" ]
}

# End-of-run report, called exactly once: the console line plus the guarded
# step-summary append, naming the mode that ran ("skipped" included) and the
# per-phase timings.
report() {
  local mode="$1" rm_s="$2" docker_s="$3"
  echo "after: $(free_mb) MB free (mode: ${mode}; rm ${rm_s}s, docker ${docker_s}s)"
  step_summary_append "$(printf 'free-disk: %s MB free (mode: %s; rm %ss, docker %ss)' \
    "$(free_mb)" "$mode" "$rm_s" "$docker_s")"
}

# One escalating cleanup: skip → parallel full delete → docker prune. Each
# phase runs only while the goal is unmet; the skip is the first branch of
# the escalation, not a special-case exit.
main() {
  echo "before: $(free_mb) MB free"

  # One escalating cleanup, goal-checked: skip (already enough) → parallel
  # full delete, per dir (largest trees first so the goal check exits
  # soonest) → docker prune. Each phase runs only while the goal is unmet.
  local mode="skipped" rm_s=0 docker_s=0 t0
  t0=$SECONDS
  for dir in "${DIRS[@]}"; do
    goal_met && break
    [ -d "$dir" ] || continue
    mode="parallel-full"
    sudo find "$dir" -mindepth 1 -maxdepth 1 -print0 \
      | sudo xargs -0 -P "$(nproc)" -n 1 rm -rf
  done
  if [ "$mode" = "parallel-full" ]; then
    rm_s=$(( SECONDS - t0 ))
  fi

  # Docker prune is conditional too: it walks the whole image store
  # serially, and on a fresh runner it frees nothing until the job has
  # pulled anything.
  if ! goal_met; then
    t0=$SECONDS
    sudo docker image prune -af >/dev/null 2>&1 || true
    docker_s=$(( SECONDS - t0 ))
  fi

  report "$mode" "$rm_s" "$docker_s"
}

# Sourcing the script (tests) defines the functions without running them.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
