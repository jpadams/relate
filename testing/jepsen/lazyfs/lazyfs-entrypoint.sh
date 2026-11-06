#!/bin/sh
# DB-node entrypoint for the lazyfs power-off lane (LABS-654): mount the
# store's directory on lazyfs, then hand PID 1 to the stock product
# supervisor — the binary under test is exactly the one the product image
# ships, only the disk under it changed.
#
# What this container is: a whole "machine" whose disk is lazyfs. The
# clear-cache fault + docker kill (the nemesis's power-off) drops every
# un-fsynced byte — lazyfs's memory dies with the container — and only
# the fsynced bytes in the backing root (/data/root, on the persistent
# volume) survive. `docker start` runs this script again: a fresh lazyfs
# mounts on the surviving backing root and serve replays the WAL from it.
#
# Environment (see lazyfs-mount.sh for the mount topology):
#   LAZYFS_MOUNT / LAZYFS_ROOT / LAZYFS_FIFO — mount knob passthrough
set -eu

/usr/local/bin/lazyfs-mount.sh

# PID 1 from here on: the stock seed-then-serve supervisor. lazyfs runs
# daemonized beside it in the same PID namespace, so a SIGKILL of the
# container takes both — and every un-fsynced byte — down together.
exec /bin/entrypoint "$@"
