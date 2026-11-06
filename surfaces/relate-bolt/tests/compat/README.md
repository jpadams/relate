# Bolt compatibility scripts

This directory holds the Bolt compatibility corpus and the harness that
replays it (ADR-0304). The scripts use the `.bolt` format of
[boltkit](https://github.com/neo4j-drivers/boltkit) (`boltkit/server/scripting.py`,
branch 4.3), with the roles turned around. boltkit's stub server plays a
script to test a driver. Here the harness is the client. It sends the
`C:` lines to a real `relate-bolt` process and checks the server's
replies against the `S:` lines.

```text
cargo nextest run -p relate-bolt --test bolt_compat
```

The test target `bolt_compat` (`tests/bolt_compat.rs`) replays every
`*.bolt` file under `scripts/`, including subdirectories. Each script gets
its own server process, started with `--uri :memory: --auth none` unless
the script has a `!: SERVER_ARGS` line. A run that has any failing script
fails once, and the panic lists every failure. `fixtures/` holds scripts
for the harness's own tests. They are deliberately wrong, so the corpus
never runs them.

To add a case, drop a `.bolt` file anywhere under `scripts/`. No Rust
change is needed.

## Example

This is `scripts/readme_example.bolt`, so it runs with the corpus. A test
keeps this block and that file identical.

```text
# The README example: a result pulled in two batches, then a failed
# query, the IGNORED that follows it, and the RESET that clears it.
!: BOLT 4.4
!: AUTO HELLO
!: AUTO GOODBYE

C: RUN "UNWIND range(1, 5) AS n RETURN n" {} {}
   PULL {"n": 3}
S: SUCCESS {"fields": ["n"]}
   RECORD [1]
   RECORD [2]
   RECORD [3]
   SUCCESS {"has_more": true}
C: PULL {"n": -1}
S: RECORD [4]
   RECORD [5]
   SUCCESS {}

C: RUN "RETURN" {} {}
S: FAILURE {"code": "*", "message": "*"}
C: RUN "RETURN 1" {} {}
S: IGNORED
C: RESET
S: SUCCESS {}
```

The harness connects and sends the magic `60 60 B0 17` with a single 4.4
proposal, and expects `00 00 04 04` back. It sends HELLO and expects
SUCCESS. Then it sends RUN and the first PULL and checks the five replies
in order: the RUN SUCCESS must list `fields: ["n"]`, and the batch
SUCCESS must say `has_more: true`. The second PULL fetches the rest. The
invalid query must fail with some `code` and `message`. The RUN after it
must be IGNORED until RESET succeeds. Last, the harness sends GOODBYE and
expects the server to close the connection.

## Lines

Each line is one of the following:

- **Blank, or starting with `#`.** Ignored. A comment must fill the whole
  line. A `#` after a message is a JSON error.
- **`!: <DIRECTIVE> [args]`.** Configures the run. See
  [Directives](#directives).
- **`C: <message or command>`.** Something the harness sends.
- **`S: <message or command>`.** Something the harness expects to receive.
- **A line with no role.** A continuation line, as in boltkit. It takes the
  role of the previous line, so one `C:` or `S:` can head a block of
  indented lines. A continuation line with no earlier role is a parse
  error.

Steps run strictly in file order. Each `C:` line is written to the socket
when it is reached. Each `S:` line reads exactly one reply, so the
harness never waits on a pipeline. Any other role marker (`X:`) is a
parse error.

## Directives

Directives belong in the header. A `!:` line after the first `C:` or `S:`
line is a parse error. Each directive may appear at most once.

| Directive | Meaning |
| --- | --- |
| `!: BOLT <major>.<minor>` | The automatic handshake proposes this version. Without the line it proposes `4.4`. The handshake sends the magic `60 60 B0 17` and one proposal `00 00 <minor> <major>`, padded with zeros to four proposals (16 bytes). It then expects that same version back. For `!: BOLT 4.4` the proposal is `00 00 04 04` and the expected reply is `00 00 04 04`. |
| `!: HANDSHAKE <hex>` | Sets the expected version reply to these 4 bytes, for example `!: HANDSHAKE 00 00 00 00` for a version the server must refuse. Any other length is a parse error. |
| `!: AUTO HELLO` | After the handshake, sends HELLO and expects any SUCCESS. The form follows the declared version: the 4.4 auth form `{"user_agent": "relate-bolt-compat/1.0", "scheme": "none"}`, or from `!: BOLT 5.4` the no-auth form `{"user_agent": "relate-bolt-compat/1.0", "bolt_agent": {"product": "relate-bolt-compat/1.0"}}` — Bolt 5.1 moved the credential to LOGON and 5.3 made `bolt_agent` mandatory (ADR-0484 decision 3). |
| `!: AUTO LOGON` | After the `!: AUTO HELLO` exchange, sends LOGON `{"scheme": "none"}` and expects any SUCCESS (ADR-0484 decision 4). It needs `!: AUTO HELLO` — LOGON answers the AUTHENTICATION state the 5.x HELLO leaves — so a script that hand-writes its HELLO, because its case rides that HELLO's extra keys or SUCCESS metadata, hand-writes the LOGON pair too. |
| `!: AUTO GOODBYE` | After the last script line, sends GOODBYE and expects the server to close the connection (as `S: <EXIT>` does). |
| `!: SERVER_ARGS <args>` | Starts the server with these flags instead of `--uri :memory: --auth none`. Args are split on whitespace, with no quoting. The harness always adds `--bind`. At least one arg is required. |

Any other directive, and any `AUTO` value other than `HELLO`, `LOGON` or
`GOODBYE` (boltkit's `AUTO RESET`, for example), is a parse error naming
the file and line.

## Commands

| Line | Meaning |
| --- | --- |
| `C: <RAW> <hex>` | Sends these bytes as they are. If this is the script's first `C:` line, no automatic handshake is sent, and the script writes its own handshake. In that case `!: BOLT`, `!: HANDSHAKE` and `!: AUTO HELLO` are parse errors, because each one needs the automatic handshake. |
| `C: <NOOP>` | Sends an empty chunk, `00 00` (a Bolt 4.1+ keep-alive). |
| `S: <RAW> <hex>` | Expects exactly these bytes next. |
| `S: <EXIT>` | Expects the server to close the connection. End of stream and a connection reset both count as closed. A reply that arrives instead is decoded into the failure report. |

Hex is written as pairs of digits. Upper and lower case are both allowed,
and whitespace between digits is ignored (`60 60 B0 17` = `6060b017`).
Other commands (boltkit's `<SLEEP>`, `S: <NOOP>`, `C: <EXIT>`) and
arguments to `<NOOP>` or `<EXIT>` are parse errors.

## Messages

A message line is `<NAME> <field> <field> ...`. Each field is one JSON
value, and fields are separated by whitespace. `C:` lines are encoded and
sent. `S:` lines are matched against the next reply.

| Side | Names |
| --- | --- |
| `C:` | `HELLO` `GOODBYE` `RESET` `RUN` `BEGIN` `COMMIT` `ROLLBACK` `DISCARD` `PULL` `ROUTE` `LOGON` `LOGOFF` `TELEMETRY` |
| `S:` | `SUCCESS` `RECORD` `IGNORED` `FAILURE` |

The `C:` names are the Bolt 4.4 set plus the 5.x additions `LOGON`
(`0x6A`), `LOGOFF` (`0x6B`) and `TELEMETRY` (`0x54`, ADR-0484). The names
are notation, not a version gate: a script may name any of them at any
negotiated version, exactly as it may write any `0xNN` tag.

Instead of a name, `0xNN` (two hex digits) gives any structure tag, for
example `C: 0x0F` or `S: 0x70 {}`. An unknown name, including a name from
the other side (`S: HELLO`), is a parse error.

### Values

JSON fields map to PackStream as follows:

| JSON | PackStream |
| --- | --- |
| integer (`1`, `-7`) | Int. A value outside the i64 range is a parse error. |
| number with a fraction or exponent (`1.0`, `1e3`) | Float |
| string | String |
| `true` / `false` | Boolean |
| `null` | Null |
| array | List |
| object | Map |
| `{"#NN": [fields]}` | Structure with hex tag `NN`, for example `{"#4E": [1, ["Person"], {}]}` for a Node |

An object whose only key starts with `#` is always a structure. Its key
must be `#` plus exactly two hex digits, and its value must be a list.
Anything else is a parse error. As a result, a map with one key that
starts with `#` cannot be written. Malformed JSON is a parse error.

### Matching

An `S:` message matches a reply when all of these hold:

- **Tags.** The structure tags are equal.
- **Fields.** The field lists match element by element, and they must have
  the same length. `S: SUCCESS` with no field does not match a SUCCESS that
  carries a metadata map. Write `S: SUCCESS {}`.
- **Maps.** An expected map matches when every listed key is present with
  a matching value. The server may send extra keys, so `{}` matches any
  map. The string `"*"` as a map value matches any value, but the key must
  still be present.
- **Lists.** Lists match element by element under these same rules, and
  must have the same length.
- **Structures inside values.** These match like messages: equal tags,
  then fields element by element.
- **Scalars.** These must be equal. An Int never equals a Float.

`"*"` is a wildcard only as a map value. As a list element or a field, it
is the literal string.

Assert only what the case is about. Summary metadata such as `type`,
`qid`, `t_first`, `t_last`, `bookmark` and `db` is better left out, or
written as `"*"`, unless the case tests that key.

## Failure reports

A failing script reports the script path and line, the line as written,
what was expected, and the decoded actual reply in the same notation.
Running out of time is also a failure: every read and write has a 10 s
timeout, so a server that never answers fails the step instead of hanging
the suite.

```text
.../tests/compat/fixtures/mismatch.bolt:10: `S: SUCCESS {"fields": ["y"]}`
  expected: SUCCESS {"fields": ["y"]}
  actual:   SUCCESS {"fields": ["x"], "columnTypes": ["INTEGER"], "t_first": 0}
```

Paths are absolute in the real output; this one is shortened.

Steps that come from a directive report that directive's line. The
handshake reports the `!: BOLT` line, or the first step when there is no
`!: BOLT` line. A parse error is reported as `path:line: <reason>`.
