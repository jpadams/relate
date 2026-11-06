"""Drive `relate` in a PTY and pin what a failed statement prints in the REPL.

A line with several statements keeps going after a failure in the interactive
shell (unlike `-c` and piped input, which stop), so the ADR-0199 progress report,
whose "N not run" clause describes a run that stopped, must not appear here.
Prints `OK` when the transcript shows the failure, the statement after it
running, and no progress line.
"""

from __future__ import annotations

import os
import re
import sys

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_repl_failure.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    pid, fd = pty_driver.fork_exec([binary, "query"])

    try:
        out = pty_driver.read_until(fd, lambda b: b"cypher>" in b, 15.0)
        if b"cypher>" not in out:
            print("FAIL: no cypher> prompt\n" + out.decode("utf-8", "replace"), file=sys.stderr)
            return 1

        # One line, three statements; the middle one is a syntax error.
        os.write(fd, b"RETURN 1 AS a; RETRN 2; RETURN 3 AS c;\r")

        def saw_third(blob: bytes) -> bool:
            # The `c` column's cell shows 3: strip the table padding once.
            compacted = blob.decode("utf-8", "replace").replace(" ", "")
            return "|c|" in compacted and "3" in compacted.split("|c|")[-1]

        out2 = pty_driver.read_until(fd, saw_third, 10.0)
        text = (out + out2).decode("utf-8", "replace")

        os.write(fd, b".quit\r")
        pty_driver.drain(fd, 1.0)

        if "syntax error" not in text:
            print("FAIL: no syntax error shown\n" + text, file=sys.stderr)
            return 1
        if "|c|" not in text.replace(" ", ""):
            print("FAIL: the statement after the failure did not run\n" + text, file=sys.stderr)
            return 1
        # The progress line's shape: "statement N of M failed; ...".
        if re.search(r"statement \d+ of \d+ failed", text):
            print("FAIL: the REPL printed a progress report\n" + text, file=sys.stderr)
            return 1
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
