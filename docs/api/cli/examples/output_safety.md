# Example: output safety

The four invocations below are copy-paste-able from the repository root and run
against an in-memory database. Human-facing output is hardened by default — no
flag enables it and none disables it; only JSON/JSONL stay byte-exact.

## (a) Table: a control character in a value is escaped

A `\u001B` escape in a string literal produces a real ESC (`0x1B`) value, which
the table formatter renders as `\u{1B}` so the terminal never sees the raw
control byte:

```bash
cargo run -p relate-cli -- :memory: "RETURN 'a\u001Bb' AS v"
```

```text
+------------+
| v          |
+------------+
| "a\u{1B}b" |
+------------+
```

Backslash, newline, tab, CR, and C1/DEL controls are escaped the same way
(`\\`, `\n`, `\t`, `\r`, `\u{XXXX}`). Markdown output applies the same rule and
additionally escapes `|` as `\|`.

## (b) CSV: controls are escaped, then formula-leading cells get a `'` guard

A CSV field is hardened in a fixed order: control characters are escaped, then
a field whose first character is `=`, `+`, `-`, `@`, or a raw CR is prefixed with
a single `'`, and finally the field is quoted per RFC 4180. A tab leading the
field is escaped to `\t` first, so it no longer triggers the guard; LF and CR
stay raw inside quotes so multi-line values round-trip. Escaping a real ESC
(`0x1B`) yields the literal text `\u{1B}`. Note the consequence on a negative
number: `-5` is emitted as the text `'-5`.

```bash
cargo run -p relate-cli -- --csv :memory: "RETURN 'a\u001Bb' AS v, '=1+1' AS f"
```

```text
v,f
a\u{1B}b,'=1+1
```

## (c) `--mode cypher`: literals stay single-line and re-parseable

Controls are escaped with the spellings the Cypher lexer understands, so the
emitted `CREATE` re-parses to exactly the original value:

```bash
cargo run -p relate-cli -- --mode cypher :memory: "RETURN 'a\u001Bb' AS v"
```

```text
CREATE ({v: 'a\u001Bb'})
```

Newline and tab keep their short forms (`\n`, `\t`) and CR becomes `\u000D`. An
alias containing a newline is sanitized before it is backticked, so it cannot
break out of the emitted statement or a `// column … omitted` comment.

## (d) `skill remove`: refuses an unmarked directory, `--force` overrides

`skill install` writes `<skills dir>/relate/.relate-installed`. A
directory that is neither marker-owned nor a recognizable relate bundle is
treated as user-authored and left untouched — `skill install` and `skill remove`
exit 1 and refuse — unless `--force` is passed. The script below pre-creates an
unmarked, unrecognizable directory in a throwaway `$HOME`:

```bash
home=$(mktemp -d)
mkdir -p "$home/.claude/skills/relate"
printf 'user-authored\n' > "$home/.claude/skills/relate/sentinel.txt"

# Refused: exit 1, directory untouched.
HOME="$home" DO_NOT_TRACK=1 cargo run -p relate-cli -- \
  skill remove --agent claude-code
# stderr:
# Error: skill: refusing to touch <home>/.claude/skills/relate: no
# .relate-installed marker present (directory was not created by
# relate); pass --force to override

# --force overrides the refusal and removes the directory.
HOME="$home" DO_NOT_TRACK=1 cargo run -p relate-cli -- \
  skill remove --agent claude-code --force

rm -rf "$home"
```

The same refusal and `--force` override apply to `skill install` for a
directory that is neither marker-owned nor a recognizable relate bundle.
`skill install` (but not `skill remove`) additionally adopts a recognizable
relate bundle — a `SKILL.md` declaring `name: relate` with a `version:`
line — without `--force`. `--agent NAME` and `--force` may appear in either
order.
