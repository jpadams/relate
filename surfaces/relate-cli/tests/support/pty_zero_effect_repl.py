"""Drive `relate` in a PTY and pin that the REPL stays free of ADR-0402 notes.

At a prompt a MATCH that finds nothing and writes nothing is routine
exploration, so neither the per-statement zero-effect note nor the `.commit`
summary belongs there (ADR-0402 Decision 1). Prints `OK` when the transcript
shows both no-ops ran and the transcript carries no `changed nothing` text.

Each line ends in a visible `RETURN` sentinel: the no-ops themselves print
nothing, so the sentinel is the proof the line ran, and waiting for it keeps
each line in its own readline call (the line editor does not buffer
type-ahead across prompts).
"""

from __future__ import annotations

import os
import sys

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_zero_effect_repl.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    pid, fd = pty_driver.fork_exec([binary, "query"])

    try:
        out = pty_driver.read_until(fd, lambda b: b"cypher>" in b, 15.0)
        if b"cypher>" not in out:
            print("FAIL: no cypher> prompt\n" + out.decode("utf-8", "replace"), file=sys.stderr)
            return 1

        def send_and_wait(line: bytes, sentinel: str) -> bytes:
            os.write(fd, line + b"\r")

            def saw(blob: bytes) -> bool:
                return sentinel.encode() in blob

            return pty_driver.read_until(fd, saw, 15.0)

        # A zero-effect write at the prompt, then the same inside a
        # transaction that is committed: both silent in the shell.
        out += send_and_wait(b"MATCH (n:Ghost) DELETE n; RETURN 1 AS ping;", "ping")
        out += send_and_wait(
            b".begin; MATCH (m:Ghost) SET m.x = 1; .commit; RETURN 2 AS pong;", "pong"
        )
        text = out.decode("utf-8", "replace")

        os.write(fd, b".quit\r")
        text += pty_driver.drain(fd, 1.0).decode("utf-8", "replace")

        if "ping" not in text or "pong" not in text:
            print("FAIL: the session did not reach its sentinels\n" + text, file=sys.stderr)
            return 1
        if "changed nothing" in text:
            print("FAIL: the REPL printed a zero-effect note\n" + text, file=sys.stderr)
            return 1
        if "note:" in text:
            print("FAIL: the REPL printed a note line\n" + text, file=sys.stderr)
            return 1
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
