#!/usr/bin/env bash
#
# check-go-examples.sh — compile and run every docs/api/go example.
#
# The Go client's documented examples are tests (AGENTS.md §2: every
# public API example runs in CI). Each file is its own `package main`, so
# the directory is not buildable as one package — the loop runs each file
# individually, which also keeps one broken example from hiding the rest.
# The directory's go.mod pins the client through a replace, the same
# standalone-project recipe clients/go/README.md documents.
#
# Usage:
#   scripts/check-go-examples.sh
# Environment:
#   CGO_LDFLAGS               link flags for a staged librelate_ffi, the
#                             grammar clients/go/README.md documents.
#                             Unset → the client's own in-checkout #cgo
#                             -L paths apply (a repo checkout with a
#                             built target/).
#   DYLD_LIBRARY_PATH /       run-time dylib resolution when linking a
#   LD_LIBRARY_PATH           cdylib staged outside the checkout (what
#                             the CI stage sets).
#
# Exits 1 when any example fails to compile or run.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT/docs/api/go/examples"

status=0
for src in *.go; do
  echo "== go example: $src"
  if ! go run "$src"; then
    echo "FAILED: $src" >&2
    status=1
  fi
done
exit "$status"
