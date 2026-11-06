#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# push-gh-pages.sh
#
# Pushes the commits a CI job made in its `gh-pages` checkout,
# rebasing them onto whatever other jobs pushed in the meantime.
#
# Several jobs write the `gh-pages` branch, and a release runs
# them in parallel: release.yml's install.sh sync (`docs`),
# publish-releases-json.yml (releases.json, ADR-0250) and
# update-website.yml (the agent-edited page, ADR-0129). Each
# checks the branch out, commits its own files and pushes. A
# plain `git push` from a checkout another job has since
# advanced is rejected — non-fast-forward, or "cannot lock ref"
# when the two pushes land together — and that file stays
# unpublished.
#
# Policy:
#   - Push HEAD to origin's gh-pages. On a rejection, fetch the
#     current remote tip and rebase ONLY this job's commits (those
#     after the tip the checkout started from) onto it, then push
#     again: at most MAX_ATTEMPTS pushes, with a linear back-off.
#     Rebasing that explicit range, not the whole branch, keeps a
#     shallow (fetch-depth: 1) checkout correct.
#   - The writers own disjoint files, so the rebase is clean. A
#     conflict means two runs changed the same file: the rebase is
#     aborted and the script fails rather than picking a winner.
#   - Never force-pushes.
#
# Usage: push-gh-pages.sh CHECKOUT_DIR
#   CHECKOUT_DIR is the gh-pages checkout (actions/checkout with
#   `ref: gh-pages`). Its `origin` remote with the persisted
#   checkout credentials is the push target, and
#   refs/remotes/origin/gh-pages marks the tip it started from.
#
# Env:
#   PUSH_GH_PAGES_BACKOFF_SECONDS
#     Back-off unit in whole seconds (default 2): the retry after
#     attempt n waits n units. Tests set 0.
#
# Exit status: 0 = pushed; 1 = refused or gave up, with a
# `::error::` diagnostic.
# ============================================================

BRANCH=gh-pages
REMOTE=origin
TRACKING_REF="refs/remotes/${REMOTE}/${BRANCH}"
MAX_ATTEMPTS=5
BACKOFF_SECONDS="${PUSH_GH_PAGES_BACKOFF_SECONDS:-2}"

if [ "$#" -ne 1 ] || [ -z "${1:-}" ]; then
  echo "::error::Usage: push-gh-pages.sh CHECKOUT_DIR"
  exit 1
fi
CHECKOUT_DIR="$1"

if ! [[ "$BACKOFF_SECONDS" =~ ^[0-9]+$ ]]; then
  echo "::error::PUSH_GH_PAGES_BACKOFF_SECONDS '${BACKOFF_SECONDS}' is not a whole number of seconds"
  exit 1
fi

if [ ! -d "$CHECKOUT_DIR" ]; then
  echo "::error::Checkout directory '${CHECKOUT_DIR}' does not exist"
  exit 1
fi
cd "$CHECKOUT_DIR"

# The tip this checkout started from; this job's commits are base..HEAD.
if ! base="$(git rev-parse --verify --quiet "${TRACKING_REF}^{commit}" 2> /dev/null)"; then
  echo "::error::'${CHECKOUT_DIR}' is not a ${BRANCH} checkout: ${TRACKING_REF} not found"
  exit 1
fi

attempt=1
while true; do
  if git push "$REMOTE" "HEAD:refs/heads/${BRANCH}"; then
    echo "[push-gh-pages] pushed $(git rev-parse --short HEAD) to ${BRANCH} (attempt ${attempt}/${MAX_ATTEMPTS})"
    exit 0
  fi

  if [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
    echo "::error::Push to ${BRANCH} still rejected after ${MAX_ATTEMPTS} attempts; giving up"
    exit 1
  fi

  echo "[push-gh-pages] push rejected (attempt ${attempt}/${MAX_ATTEMPTS}); rebasing onto the current ${REMOTE}/${BRANCH}"
  sleep "$((attempt * BACKOFF_SECONDS))"

  if ! git fetch "$REMOTE" "+refs/heads/${BRANCH}:${TRACKING_REF}"; then
    echo "::error::Could not fetch ${REMOTE}/${BRANCH} to rebase onto"
    exit 1
  fi
  if ! git rebase --onto "$TRACKING_REF" "$base"; then
    git rebase --abort
    echo "::error::Rebasing onto the current ${REMOTE}/${BRANCH} conflicts: another run changed the same files; refusing to pick a winner"
    exit 1
  fi
  base="$(git rev-parse "$TRACKING_REF")"
  attempt=$((attempt + 1))
done
