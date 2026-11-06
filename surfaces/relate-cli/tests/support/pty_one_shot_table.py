"""Drive a one-shot `relate` query with stdout attached to a PTY.

With stdout a terminal the CLI's default output mode is the human-facing
table (ADR-0107), so `RETURN 1 AS n` must render a `+---+` bordered table
containing the cell `1`. Prints `OK` when a table border/cell is seen.
"""

from __future__ import annotations

import sys

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_one_shot_table.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    pid, fd = pty_driver.fork_exec([binary, "-c", "RETURN 1 AS n"])

    def has_table(blob: bytes) -> bool:
        text = blob.decode("utf-8", "replace")
        compacted = text.replace(" ", "")
        # Border row plus the value cell ``|1|``; not JSON ``{"n":1}``.
        return "+" in text and "|" in text and "|1|" in compacted

    try:
        out = pty_driver.read_until(fd, has_table, 15.0)
        if not has_table(out):
            print(
                "FAIL: no table output\n" + out.decode("utf-8", "replace"),
                file=sys.stderr,
            )
            return 1
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
