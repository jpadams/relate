"""Drive bare `relate` (no arguments) in a PTY and pin the ADR-0231 shell.

With no statement and no command word, an interactive terminal opens the same
statement-session shell as `relate query`: the banner, the `cypher>`
prompt, statement execution, and a clean 0 exit on `.quit`. Prints `OK` when
the transcript shows all of it.
"""

from __future__ import annotations

import os
import sys

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_bare_shell.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    # No arguments at all: the bare form must open the shell on a TTY.
    pid, fd = pty_driver.fork_exec([binary])

    try:
        out = pty_driver.read_until(fd, lambda b: b"cypher>" in b, 15.0)
        if b"cypher>" not in out:
            print(
                "FAIL: bare invocation showed no cypher> prompt\n"
                + out.decode("utf-8", "replace"),
                file=sys.stderr,
            )
            return 1
        if b"Relate" not in out:
            print(
                "FAIL: no shell banner\n" + out.decode("utf-8", "replace"),
                file=sys.stderr,
            )
            return 1

        os.write(fd, b"RETURN 42 AS the_answer;\r")
        out2 = pty_driver.read_until(
            fd, lambda b: b"the_answer" in b and b"42" in b, 10.0
        )
        text = (out + out2).decode("utf-8", "replace")
        if "the_answer" not in text or "42" not in text:
            print("FAIL: the statement did not run\n" + text, file=sys.stderr)
            return 1

        os.write(fd, b".quit\r")
        pty_driver.drain(fd, 2.0)
        code = pty_driver.expect_clean_exit(pid, 5.0)
        if code is None:
            print("FAIL: shell did not exit on .quit", file=sys.stderr)
            return 1
        if code != 0:
            print(f"FAIL: shell exit code {code}", file=sys.stderr)
            return 1
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
