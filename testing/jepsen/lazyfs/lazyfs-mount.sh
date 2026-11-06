#!/bin/sh
# The one lazyfs mount helper for the power-off lane (LABS-654): used by
# both the DB node's entrypoint and the host probe — one policy, one
# spelling (LABS-311: one canonical helper per policy).
#
# Mounts lazyfs (VLDB'24, dsrhaslab/lazyfs) over LAZYFS_MOUNT with
# LAZYFS_ROOT as the backing root:
#
#   LAZYFS_MOUNT   what serve sees — power-volatile: un-fsynced bytes live
#                  only in lazyfs's userspace memory
#   LAZYFS_ROOT    the backing root — the "disk": only fsynced bytes ever
#                  reach it
#
# The mount is not trusted on the mount script's word: this helper waits
# until the mount is visible in /proc/mounts AND the fault FIFO exists
# before returning. A serve that started against an unmounted directory
# would write the store straight to the container layer — a lane that
# looks green while testing nothing, exactly the silent-no-op failure mode
# this harness refuses everywhere.
#
# Environment (defaults must match the baked config/default.toml's
# fifo_path):
#   LAZYFS_MOUNT   default /mnt/lazyfs
#   LAZYFS_ROOT    default /data/root
#   LAZYFS_FIFO    default /tmp/faults.fifo
set -eu

LAZYFS_MOUNT="${LAZYFS_MOUNT:-/mnt/lazyfs}"
LAZYFS_ROOT="${LAZYFS_ROOT:-/data/root}"
LAZYFS_FIFO="${LAZYFS_FIFO:-/tmp/faults.fifo}"

if [ ! -c /dev/fuse ]; then
    echo "lazyfs-mount: no /dev/fuse in this container — the lane needs a" \
         "FUSE-capable host (run-poweroff.sh probes it first)" >&2
    exit 1
fi

# A stale FIFO from this container's previous life breaks lazyfs's mkfifo
# on remount (the writable layer survives docker kill/start); the verified
# probe recipe removes it before every mount.
rm -f "$LAZYFS_FIFO"

mkdir -p "$LAZYFS_MOUNT" "$LAZYFS_ROOT"

cd /opt/lazyfs/lazyfs
# -s (single-thread) mirrors the verified probe recipe. In this mode the
# script daemonizes lazyfs itself and returns, so the waits below — not
# the script's exit — are the mount-ready signal.
./scripts/mount-lazyfs.sh -c config/default.toml \
    -m "$LAZYFS_MOUNT" -r "$LAZYFS_ROOT" -s

# Mount visible in /proc/mounts, or fail loudly (30s budget).
i=0
until grep -qs " $LAZYFS_MOUNT " /proc/mounts; do
    i=$((i + 1))
    if [ "$i" -gt 300 ]; then
        echo "lazyfs-mount: lazyfs never mounted on $LAZYFS_MOUNT" \
             "(see the container log for its output)" >&2
        exit 1
    fi
    sleep 0.1
done

# The nemesis's fault channel must exist before serve starts.
if [ ! -p "$LAZYFS_FIFO" ]; then
    echo "lazyfs-mount: fault FIFO $LAZYFS_FIFO missing after mount" >&2
    exit 1
fi

echo "lazyfs-mount: $LAZYFS_MOUNT mounted on backing root $LAZYFS_ROOT," \
     "fault FIFO $LAZYFS_FIFO"
