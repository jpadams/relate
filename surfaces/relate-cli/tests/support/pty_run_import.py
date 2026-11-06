"""Drive `run <importer>` on a PTY and assert whether the shell opens.

``exit``: the process must finish without a ``cypher>`` prompt (explicit
``--database``, ADR-0286). ``prompt``: the process must reach ``cypher>``
after the import summary (omitted ``--database``).

The importer packages must be installed under the given
``$XDG_CONFIG_HOME`` root — the test installs them and passes the root
here, and this driver pins it into the child's environment on top of
``pty_driver``'s isolation defaults (the registry reads the machine
state the test named, never the machine running the test).

usage: pty_run_import.py exit|prompt BINARY XDG_DIR [args...]
"""

from __future__ import annotations

import sys

import pty_driver


def main() -> int:
    if len(sys.argv) < 5 or sys.argv[1] not in ("exit", "prompt"):
        print(
            "usage: pty_run_import.py exit|prompt BINARY XDG_DIR [args...]",
            file=sys.stderr,
        )
        return 2

    mode = sys.argv[1]
    binary = sys.argv[2]
    xdg = sys.argv[3]
    argv = [binary, *sys.argv[4:]]

    pid, fd = pty_driver.fork_exec(argv, {"XDG_CONFIG_HOME": xdg})
    try:
        out = pty_driver.read_until(fd, lambda blob: b"cypher>" in blob, 20.0)
        text = out.decode("utf-8", "replace")
        saw_prompt = b"cypher>" in out
        if "imported" not in text:
            print(f"FAIL: import summary missing\n{text}", file=sys.stderr)
            return 1
        if mode == "prompt":
            if not saw_prompt:
                print(f"FAIL: expected the shell after import\n{text}", file=sys.stderr)
                return 1
            print("OK")
            return 0
        if saw_prompt:
            print(f"FAIL: interactive prompt opened\n{text}", file=sys.stderr)
            return 1
        code = pty_driver.expect_clean_exit(pid, 5.0)
        if code is None:
            print(f"FAIL: process did not exit\n{text}", file=sys.stderr)
            return 1
        if code == 0:
            print("OK")
            return 0
        print(f"FAIL: exit code {code}\n{text}", file=sys.stderr)
        return 1
    finally:
        pty_driver.cleanup(pid, fd)


if __name__ == "__main__":
    sys.exit(main())
