#!/usr/bin/env bash
# ADR-0251 §4 fail-fast ordering: print the failed job names of the most
# recent completed CI run on the current branch, one per line, so the
# `failfast` job can annotate them and re-run the cheap repros first.
#
# Fresh branches, renamed checks, and API errors degrade to empty output
# and exit 0 — this helper never fails a run.
#
# Environment: GH_TOKEN (PAT or GITHUB_TOKEN), GITHUB_REPOSITORY, and
# GITHUB_HEAD_REF (pull_request) or GITHUB_REF_NAME (push). The `gh` CLI is
# preinstalled on GitHub-hosted runners.
set -uo pipefail

if [[ -z "${GH_TOKEN:-}" || -z "${GITHUB_REPOSITORY:-}" ]]; then
  exit 0
fi
branch="${GITHUB_HEAD_REF:-${GITHUB_REF_NAME:-}}"
if [[ -z "$branch" ]]; then
  exit 0
fi

run_id="$(gh api "repos/${GITHUB_REPOSITORY}/actions/workflows/ci.yml/runs?branch=${branch}&status=completed&per_page=5" \
  --jq '[.workflow_runs[] | select(.id != '"${GITHUB_RUN_ID:-0}"')][0].id // empty' 2>/dev/null)" || exit 0
if [[ -z "$run_id" ]]; then
  exit 0
fi

gh api "repos/${GITHUB_REPOSITORY}/actions/runs/${run_id}/jobs?filter=latest&per_page=100" \
  --jq '.jobs[] | select(.conclusion == "failure") | .name' 2>/dev/null || true
