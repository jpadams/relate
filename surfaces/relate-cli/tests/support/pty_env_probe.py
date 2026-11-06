"""Drive `SHOW PROCEDURES` on a PTY and pin the closed preview gate.

The PTY suites spawn the binary from Python, so the ambient scrub has to run
on this process's environment: `tests/support/mod.rs` does it in
`pty_command`, before `pty_driver.fork_exec` copies `os.environ` into the
child. With `RELATE_PREVIEW` inherited, the child would list the gated
`db.ontology.*` rows. Prints `OK` only when they are absent and the catalog
still rendered `db.labels`.

usage: pty_env_probe.py PATH_TO_RELATE
"""

from __future__ import annotations

import sys

import pty_driver


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: pty_env_probe.py PATH_TO_RELATE", file=sys.stderr)
        return 2

    binary = sys.argv[1]
    pid, fd = pty_driver.fork_exec(
        [
            binary,
            "query",
            "SHOW PROCEDURES YIELD name",
            "--mode",
            "jsonl",
            "-d",
            ":memory:",
        ]
    )
    try:
        out = pty_driver.drain(fd, 20.0)
        text = out.decode("utf-8", "replace")
        if "db.ontology." in text:
            print(
                "FAIL: an inherited preview opt-in opened the gate\n" + text,
                file=sys.stderr,
            )
            return 1
        if "db.labels" not in text:
            print(
                "FAIL: the procedure catalog did not render\n" + text,
                file=sys.stderr,
            )
            return 1
        code = pty_driver.expect_clean_exit(pid, 5.0)
        if code is None:
            print("FAIL: the query did not exit\n" + text, file=sys.stderr)
            return 1
        if code != 0:
            print(f"FAIL: the query exited {code}\n" + text, file=sys.stderr)
            return 1
        print("OK")
        return 0
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
