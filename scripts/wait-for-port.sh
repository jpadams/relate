#!/usr/bin/env bash
# Wait until a TCP port accepts connections, with a deadline.
#
# Replaces the CI smoke legs' startup guess: `sleep 1` proves nothing and
# `kill -0` only proves the process is alive, not listening — a server
# still initializing passes both (LABS-622). Polls a bash /dev/tcp
# connect once per second; exits 0 as soon as the port accepts, 1 with a
# loud message when the deadline passes.
#
# Usage: bash scripts/wait-for-port.sh HOST PORT [TIMEOUT_SECS]
# TIMEOUT_SECS defaults to 30.
set -euo pipefail

host="${1:?usage: wait-for-port.sh HOST PORT [TIMEOUT_SECS]}"
port="${2:?usage: wait-for-port.sh HOST PORT [TIMEOUT_SECS]}"
timeout_secs="${3:-30}"

deadline=$((SECONDS + timeout_secs))
until (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; do
  if (( SECONDS >= deadline )); then
    echo "wait-for-port: ${host}:${port} still not accepting connections after ${timeout_secs}s" >&2
    exit 1
  fi
  sleep 1
done
