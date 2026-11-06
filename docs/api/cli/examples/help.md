# Example: help and usage errors

Top-level `relate --help` (or `-h`) prints usage to stdout and exits 0:
two usage lines, a `Commands:` index of every command compiled into the
binary, the query-run options, and a closing
`Run 'relate <COMMAND> --help' for details on a command.` line.

```bash
relate --help
```

A command's own help is `relate <command> --help` (or `-h`); it prints
that command's synopsis and flags to stdout and exits 0.

```bash
relate clone --help
```

A usage error is scoped to the command that failed. `clone` requires both
`SOURCE` and `DEST`, so passing only one prints three lines to stderr and exits
2 (a mistyped flag adds a fourth, `Did you mean …?`, below):

```bash
relate clone onlyone
```

```text
Error: clone requires SOURCE and DEST
Usage: relate clone [--wal-dir DIR] [--to-snapshot-format F] SOURCE DEST
Run 'relate clone --help' for more information.
```

A top-level parse error uses the query-run synopsis and the top-level help
hint; `--bogus` is an unknown flag, so it exits 2 with three lines too:

```bash
relate --bogus
```

```text
Error: unknown flag: --bogus
Usage: relate [OPTIONS] [--database PATH] [QUERY]
Run 'relate --help' for more information.
```

A mistyped flag adds a `Did you mean` line after the error when a real flag is
close, for the query run and for each command's own flags. A word that is not
close to any flag gets none:

```bash
relate --readonly :memory: "RETURN 1"
relate clone --wal-dri ./src ./dst
```

```text
Error: unknown flag: --readonly
Did you mean '--read-only'?
Usage: relate [OPTIONS] [--database PATH] [QUERY]
Run 'relate --help' for more information.
```

```text
Error: unexpected argument to clone: --wal-dri
Did you mean '--wal-dir'?
Usage: relate clone [--wal-dir DIR] [--to-snapshot-format F] SOURCE DEST
Run 'relate clone --help' for more information.
```
