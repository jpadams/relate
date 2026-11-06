"""Drive `relate` in a PTY and assert left-arrow edits the line.

Without a line editor, CSI sequences such as left-arrow (`ESC [ D`) are
appended to the buffer and the query does not become `RETURN 42 AS x;`.
With rustyline they move the cursor, so deleting the `3` in `43` and
typing `2` submits `RETURN 42 AS x;`.
"""

from __future__ import annotations

import os
import sys
import time

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_line_edit.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    pid, fd = pty_driver.fork_exec([binary, "query"])

    try:
        out = pty_driver.read_until(fd, lambda b: b"cypher>" in b, 15.0)
        if b"cypher>" not in out:
            print("FAIL: no cypher> prompt\n" + out.decode("utf-8", "replace"), file=sys.stderr)
            return 1

        # RETURN 43 AS x;  then left onto the '3', Delete, type '2', Enter.
        os.write(fd, b"RETURN 43 AS x;")
        time.sleep(0.05)
        os.write(fd, b"\x1b[D" * 7)
        time.sleep(0.05)
        os.write(fd, b"\x1b[3~")
        time.sleep(0.05)
        os.write(fd, b"2\r")

        # Wait for the result *cell* (`| 42 |` → `|42|`), not the edited
        # prompt (`RETURN 42 AS x;`) or the `| x |` header. Those show up
        # first and made CI return before the table body was printed.
        def has_result_cell(blob: bytes) -> bool:
            compacted = blob.decode("utf-8", "replace").replace(" ", "")
            return "|42|" in compacted

        out2 = pty_driver.read_until(fd, has_result_cell, 10.0)
        transcript = out + out2
        text = transcript.decode("utf-8", "replace")
        if not has_result_cell(transcript):
            print("FAIL: no table cell 42\n" + text, file=sys.stderr)
            return 1

        os.write(fd, b".quit\r")
        pty_driver.drain(fd, 1.0)
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
