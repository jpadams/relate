"""Shared PTY scaffolding for the relate CLI test drivers.

Runs a child process attached to a PTY with a forced real-terminal
environment (cargo test / CI often set TERM=dumb), reads from the master
fd until a predicate matches, and cleans up by killing the child and
closing the fd. Test-specific scripts import this module; a same-dir
``import pty_driver`` works because Python puts the script's directory on
``sys.path`` when run as ``python3 path/to/script.py``.
"""

from __future__ import annotations

import errno
import os
import pty
import select
import signal
import tempfile
import time
from collections.abc import Callable


def fork_exec(
    argv: list[str],
    extra_env: dict[str, str] | None = None,
) -> tuple[int, int]:
    """Fork a PTY and exec ``argv`` in the child; return ``(pid, master_fd)``.

    Forces ``TERM=xterm`` and an 80x24 window so cargo test / CI (which
    often set ``TERM=dumb``) still exercise the real terminal paths, and
    isolates telemetry atomically with the spawn: scratch ``HOME``,
    tracking off, sinkholed endpoint, no config/env overrides — no child
    this driver forks can report the host's telemetry config.

    ``extra_env`` is applied on top of the isolation defaults: a driver
    whose CLI invocation names its own machine state (the extension
    registry's ``$XDG_CONFIG_HOME``, pinned to the test's install root by
    ``pty_run_import.py``) sets exactly what it names; the telemetry
    isolation above is never overridable through it.

    When spawned through the Rust harness (``pty_command`` in
    ``tests/support/mod.rs``) the ambient agent-harness, ``RELATE_MAX_ROWS``
    and ``RELATE_PREVIEW`` variables are already removed from this
    process, so the ``os.environ`` copy below cannot leak them into the
    child. That scrub list has one definition on the Rust side; do not add
    a second copy here.
    """
    env = os.environ.copy()
    env["TERM"] = "xterm"
    env["COLUMNS"] = "80"
    env["LINES"] = "24"
    # Telemetry isolation is atomic with the spawn: scratch HOME, tracking
    # off, sinkholed endpoint, no config/env overrides. A driver cannot
    # fork an un-isolated child, so no test's outcome depends on the
    # host's telemetry config.
    env["HOME"] = tempfile.mkdtemp(prefix="relate-pty-")
    env.pop("XDG_CONFIG_HOME", None)
    env.pop("RELATE_TELEMETRY", None)
    env["DO_NOT_TRACK"] = "1"
    env["RELATE_MIXPANEL_ENDPOINT"] = "http://127.0.0.1:1"
    if extra_env:
        env.update(extra_env)

    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(argv[0], argv, env)
    return pid, fd


def read_until(fd: int, pred: Callable[[bytes], bool], timeout: float) -> bytes:
    """Read from the PTY master fd until ``pred`` matches or ``timeout``.

    Returns everything read so far, including when the deadline expires
    without a match. A PTY master read raises ``EIO`` once the child side
    closes; that is a normal EOF here, not an error.
    """
    buf = bytearray()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        remaining = max(0.0, deadline - time.monotonic())
        ready, _, _ = select.select([fd], [], [], remaining)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 4096)
        except OSError as e:
            if e.errno == errno.EIO:
                break
            raise
        if not chunk:
            break
        buf.extend(chunk)
        if pred(bytes(buf)):
            return bytes(buf)
    return bytes(buf)


def wait_status(pid: int, timeout: float) -> int | None:
    """Reap ``pid`` if it exits within ``timeout``; otherwise return None.

    The one reap helper for every driver: ``cleanup`` swallows the OSError
    from signaling an already-reaped child, so a driver that waited here can
    still call ``cleanup`` unconditionally in its ``finally``.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        wpid, status = os.waitpid(pid, os.WNOHANG)
        if wpid == pid:
            return status
        time.sleep(0.05)
    return None


def drain(fd: int, timeout: float) -> bytes:
    """Read the PTY master until EOF or ``timeout`` — give the child up to
    ``timeout`` seconds to close its side, discarding nothing (returned for
    callers that want the tail of the transcript)."""
    return read_until(fd, lambda _b: False, timeout)


def expect_clean_exit(pid: int, timeout: float) -> int | None:
    """Reap ``pid`` within ``timeout`` and return its exit code (negative
    for a signal exit, per ``os.waitstatus_to_exitcode``); None on timeout.
    Drivers keep only their own failure messages around this one call."""
    status = wait_status(pid, timeout)
    if status is None:
        return None
    return os.waitstatus_to_exitcode(status)


def cleanup(pid: int, fd: int) -> None:
    """Best-effort kill of the child and close of the master fd."""
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass
    try:
        os.close(fd)
    except OSError:
        pass
