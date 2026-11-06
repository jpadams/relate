# Example: one-shot query

The explicit form (ADR-0200) is `query` with an optional `--database`:

```bash
cargo run -p relate-cli -- query "RETURN 1 AS n" --database :memory:
```

Expected stdout on a terminal is a table with a header `n` and a row `1`.

The path-first form uses the same flag:

```bash
cargo run -p relate-cli -- --database :memory: -c "RETURN 1 AS n"
```

Piping or redirecting stdout switches the default to JSONL (ADR-0107) — or
TOON when agent mode is in effect (ADR-0426), which is the case inside a
harness like Claude Code. The example pins the object-per-line default so it
holds under any agent detection:

```bash
cargo run -p relate-cli -- query --mode jsonl "RETURN 1 AS n" | cat
```

Expected stdout is `{"n":1}`.
