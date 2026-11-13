# CLI feedback reports

## Install

Nothing extra: the verbs ride the `relate` binary's `feedback` feature (in the
`full` release binary and the `agent` bundle). Check with `relate --help` —
`feedback` appears in `Commands:` when the feature is compiled in.

## Minimal usage

A report is person-consented: a human grants the authorization once, and it
lasts 30 days. Ask the person to run:

```bash
relate feedback enable     # authorize reports for 30 days
relate feedback status     # enabled (until 2026-12-12T12:00:00Z)
```

Then report one friction point — what you expected and what actually happened:

```bash
relate feedback submit \
  --category unclear_error \
  --expected "the info command to print the version" \
  --observed "it printed help instead" \
  --command info
```

`submit` prints one JSON outcome line on stdout: `{"outcome": ..., "reason":
..., "request_id": ..., ...}`. Exit codes: **0** when the report was sent or
skipped (feedback not enabled, telemetry opted out), **1** when it was locally
rejected or the send failed, **2** for a usage error. A locally rejected report
records no payload in the local receipt.

```bash
relate feedback disable    # revoke; later submits send nothing
```

## Agent flow: draft, approval, send

A report is the person's decision, never yours. On friction in a session — a
command that failed, a wrong expectation, a missing capability — follow these
steps in this order:

1. **Draft.** Write a concise report of the friction: the observed problem,
   the failed attempts it caused, and the improvement you suggest.
2. **Ask.** Show the draft to the person and ask for explicit approval.
3. **Send — only after that approval.** An unapproved draft is dropped, never
   sent.

The send itself is a call to the `feedback_submit` tool: an MCP host launches
the hidden `relate feedback serve` verb (newline-delimited JSON-RPC 2.0 on
the process's stdio), and the tool takes `category`, `expected`, `observed`
and an optional `command`. **The call is the send and carries no permission
of its own** — the consent it checks is the person's recorded `feedback
enable` authorization, not their approval of *this* draft. Hold the person's
explicit approval before every call; an unapproved draft is never sent. The
shell's `relate feedback submit` runs the same pipeline, so it obeys the same
flow.

## Gotchas

- **Consent first, always.** Without `enable` (or after `disable`) every
  `submit` is a local `skipped` outcome with reason `not_enabled`, prints the
  JSON document, exits 0, and sends nothing. `DO_NOT_TRACK=1` and
  `RELATE_TELEMETRY=0` suppress sending the same way.
- **Free text is validated and redacted before any send.** Emails, URLs, host
  names, IP addresses, file paths, data filenames, secrets, Cypher snippets
  and pasted output are rejected whole (never rewritten); the one exception
  is a documentation link to this project's own docs or repository, by path
  (`relatedb.ai/<path>`, `github.com/relatedb/relate/<path>`), or to the
  Neo4j Cypher manual (`neo4j.com/docs/`). A bare host URL with no path
  (`relatedb.ai`, `github.com/relatedb/relate`) is refused. `--expected` and
  `--observed` are at most 255 bytes.
- **Optional fields are dropped, not refused.** `--command` survives only for
  a visible command path; `--error-code` only for a code name in the CLI's
  exit table (`fatal_error`, `usage_error`, `conflict`, `validation_error`,
  `timeout`); `--model` only for a lowercase provider-prefixed label (at most
  40 chars). The outcome's `dropped_fields` names what was removed.
- **Nothing is queued.** A report is one synchronous POST with a 2-second
  deadline and no retries; `--request-id` lets a retry keep the same id (and
  the `$insert_id` dedup key) after `failed` or `unknown`.
- **Endpoint override is loopback-or-Mixpanel.** `RELATE_FEEDBACK_ENDPOINT`
  accepts `https://` on the Mixpanel hosts, or `http://` on loopback only; any
  other value is refused. `feedback enable` reports the refusal (exit 1) and
  records no authorization, and with no destination configured a report ends
  `failed` with reason `not_configured` instead of being sent elsewhere.
  `feedback enable` records the destination it resolves, and later reports go
  there: changing the override afterwards does not divert them, and a
  different destination needs a new `feedback enable`.
- **Local receipts** — `feedback-receipts.jsonl` in the config directory
  (`$XDG_CONFIG_HOME/relate/` or `~/.config/relate/`) records every rejection
  and send attempt.

Full contract: `docs/api/cli/spec.md` (flag tables, exit codes, environment,
state files).
