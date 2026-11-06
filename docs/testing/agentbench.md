# Agent A/B harness (`relate-agentbench`)

The headless agent A/B harness (ADR-0404 principle 5, LABS-481 G-3): it
measures what a coding agent actually **reads back** from the Relate
CLI's output, per output mode — DuckDB's Agent Mode experiment
methodology, adapted.

## What it measures

Each question × arm × repetition runs one headless Claude Code session
(`claude -p`) that answers a question using the `relate` CLI. The
arms differ only in the PATH wrapper the session's `relate`
resolves to: every wrapper injects one `--mode` flag — `baseline` pins
the pre-ADR-0426 machine default (`jsonl`), every other arm its mode
(e.g. `toon`). The agent never knows which arm it is on.

The measured environment is defined: both the seeding spawn and the
headless session run under the workspace's harness child-env policy
(ambient `RELATE_*` knobs scrubbed, telemetry opted out), so the
arms differ only in the wrapper-injected `--mode` — never in a leaked
ambient knob.

Per run the harness records:

- **tool-output bytes** — the metric: the sum of every Bash
  tool_result's content the agent read (what DuckDB measured as −59%
  for their compact mode)
- **input / output tokens** — the session's totals (input is dominated
  by the context the model re-reads each turn; the tool-output bytes
  are the part the CLI controls)
- **turns**, **wall seconds**, and **correctness** — the honesty
  clause: a cheaper arm that answers wrong loses

## Usage

```bash
cargo build -p relate-cli
RELATE_BIN=target/debug/relate cargo run -p relate-agentbench -- \
  --questions testing/relate-agentbench/questions/basic.json \
  --database /tmp/agentbench.rl \
  --arms baseline,toon \
  --reps 3
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--questions FILE` | required | the questions file (JSON array: `id`, `prompt`, `setupCypher`, `expectedAnswer`) |
| `--database PATH` | required | the database directory, recreated before every measured run (the agent's Bash tool is unrestricted, so each run must start from identical state; the setup runs through the real binary, never a wrapper) |
| `--arms LIST` | `baseline,toon` | comma-separated arms; `baseline` pins `--mode jsonl` (the pre-ADR-0426 default — a pass-through would resolve to the harness sessions' own detected default under ADR-0426), any other entry is a `--mode` value its wrapper injects — validated against the CLI's own `--mode` vocabulary at parse, so a typo is a usage error, never a broken wrapper or a wasted measured session |
| `--reps N` | 1 | repetitions per question × arm; the report shows p25/median/p75 distributions (at least 1 — zero runs are not a measurement) |
| `--max-turns N` | 30 | the headless session's turn cap |
| `--claude PATH` | `claude` on PATH | the headless `claude` binary |
| `--relate PATH` | the `target/` build | the binary the wrappers exec |

## Reading the report

The report is one row per arm: every metric as a p25/median/p75
distribution (the run-to-run spread is often larger than the arms'
differences on small results, so a bare median hides the shape that
decides) plus a correct/total count. The correctness gate is word-level, never substring: the
expected answer must appear as a whole word of the session's final
text (so `6` does not mark `16` correct, nor `P1` the `P120` listing)
— every question's `expectedAnswer` must be word-exact under that
gate. The decision rule (ADR-0404 principle 5): a default flips only
on a **material** tool-output-bytes win with correctness held; token
wins that cost turns or wall time are reported honestly as non-wins,
the shape DuckDB published.

The question set grows from the cookbook's task shapes; add questions
to `questions/` rather than lengthening prompts.
