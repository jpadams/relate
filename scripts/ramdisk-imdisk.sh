#!/usr/bin/env bash
# ImDisk ramdisk for the Windows lanes (ADR-0251 §2): NTFS metadata I/O
# dominates the Windows build/test wall-clock, so the target dir and the
# temp dirs go on an ImDisk ramdisk (8 GB of the runner's 16 GB). Any
# driver-install or mount failure falls back to the plain disk paths with a
# warning — the step summary records which mode ran. `line-tables-only`
# keeps the debug target inside the ramdisk (backtraces stay symbolized).
#
# The env vars reach later steps through $GITHUB_ENV, so this script runs
# as a normal (non-sourced) step; used by ci.yml's Windows lanes and by
# background-cadences.yml's os-sweep lane.
set -euo pipefail

mode="disk"
if choco install imdisk-toolkit -y --no-progress --limit-output; then
  export PATH="$PATH:/c/Program Files/ImDisk"
  if imdisk -a -s 8G -m R: -p "/fs:ntfs /q" && [[ -d /r ]]; then
    mkdir -p /r/target /r/tmp
    {
      echo "CARGO_TARGET_DIR=R:\\target"
      echo "TMP=R:\\tmp"
      echo "TEMP=R:\\tmp"
      echo "CARGO_PROFILE_DEV_DEBUG=line-tables-only"
    } >> "$GITHUB_ENV"
    mode="imdisk"
  fi
fi
echo "ramdisk mode: **$mode**" >> "$GITHUB_STEP_SUMMARY"
if [[ "$mode" != "imdisk" ]]; then
  echo "::warning::ImDisk ramdisk unavailable; running on the plain disk paths"
fi
