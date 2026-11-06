#!/bin/sh
# The lane's host probe (LABS-654): the verified runner-probe step from
# the lazyfs recipe, run INSIDE the db-lazyfs image before every lane
# invocation. It certifies THIS host's fault semantics, not just the
# device node:
#
#   1. /dev/fuse is reachable in a privileged container,
#   2. fsynced bytes reach the lazyfs backing root,
#   3. un-fsynced bytes never do,
#   4. the clear-cache fault + SIGKILL of lazyfs drops exactly the
#      un-fsynced bytes and keeps exactly the fsynced ones.
#
# A host where FUSE exists but behaves differently must fail the lane
# loudly HERE, not produce a green durability run built on a fault that
# never dropped anything.
#
# Runs standalone (no compose, no store, no serve): docker run --rm
# --privileged --device /dev/fuse --entrypoint /usr/local/bin/lazyfs-probe.sh
# relate-jepsen-db-lazyfs:latest — run-poweroff.sh wraps exactly that.
set -eu

MNT=/tmp/probe.mnt
ROOT=/tmp/probe.root
# The fault FIFO: lazyfs-mount.sh's LAZYFS_FIFO is the canonical
# definition (its default matches the baked config/default.toml). The
# literal repeats here because POSIX sh cannot import another script's
# constant, so a drift guard makes the repetition loud instead of
# silent: the mount helper must still carry the same default.
LAZYFS_FIFO="${LAZYFS_FIFO:-/tmp/faults.fifo}"
grep -q 'LAZYFS_FIFO:-/tmp/faults.fifo' /usr/local/bin/lazyfs-mount.sh || {
    echo "lazyfs-probe: LAZYFS_FIFO default drifted from lazyfs-mount.sh" >&2
    exit 1
}

LAZYFS_MOUNT="$MNT" LAZYFS_ROOT="$ROOT" LAZYFS_FIFO="$LAZYFS_FIFO" \
    /usr/local/bin/lazyfs-mount.sh

# The probe payload: one fsynced prefix, one un-fsynced whole file.
printf 'KEEP-' > "$MNT/keep.txt"
sync -d "$MNT/keep.txt"
printf 'UNSYNCED-LOST-BYTES' > "$MNT/drop.txt"

# The power-off, exactly as the nemesis delivers it: the clear-cache
# fault (a blocking FIFO write — it completes only once lazyfs has read
# it), the processing settle, then SIGKILL of lazyfs.
echo 'lazyfs::clear-cache' > "$LAZYFS_FIFO"
sleep 1
pkill -9 -f libpcache_lazyfs || true
pkill -9 -f 'build/lazyfs' || true
sleep 1
fusermount3 -uz "$MNT" 2>/dev/null || fusermount -uz "$MNT" 2>/dev/null \
    || umount -l "$MNT" 2>/dev/null || true

# The verdict, read from the backing root only.
FAIL=0
if [ "$(cat "$ROOT/keep.txt" 2>/dev/null)" = 'KEEP-' ]; then
    echo 'probe: OK — fsynced bytes survived the power-off'
else
    echo "probe: FAIL — fsynced bytes lost: keep.txt => '$(cat "$ROOT/keep.txt" 2>/dev/null)'" >&2
    FAIL=1
fi
if [ -s "$ROOT/drop.txt" ]; then
    echo "probe: FAIL — un-fsynced bytes reached the backing root: drop.txt => '$(cat "$ROOT/drop.txt")'" >&2
    FAIL=1
else
    echo 'probe: OK — un-fsynced bytes dropped by the power-off'
fi

if [ "$FAIL" -ne 0 ]; then
    echo 'probe: VERDICT — HOST SEMANTICS WRONG, the lane would be vacuous' >&2
    exit 1
fi
echo 'probe: VERDICT — RUNNER PROBE PASSED'
