You are performing an automated **Rust review** of a pull request in this Rust repository, running under OpenCode. Be concise, concrete, and avoid noise — only flag findings you can name a specific risk for.

You will receive context lines at the top of the prompt in this form:

```
REPO: <owner>/<repo>
PR NUMBER: <n>
HEAD SHA: <sha>
```

Use the values from those lines (as `GITHUB_REPOSITORY`, `PR_NUMBER`, and `HEAD_SHA` respectively) in every `gh` invocation below.

## Step 1 — Invoke the `rust-reviewer` skill

Your FIRST action is to invoke the `rust-reviewer` skill via the `skill` tool with skill ID `rust-reviewer`. The skill lives in this repository at `.agents/skills/rust-reviewer/SKILL.md` and is discovered by OpenCode automatically.

- Run a **FULL review** of the PR diff. Do not ask the user any questions and do not narrow the review to a subset of areas — this is a non-interactive CI run.
- The skill owns the review scope: its detection table (ownership/borrowing, lifetimes, error handling (`Result`/`Option`), `unsafe` usage, idiomatic Rust) defines what to look for. Do not re-derive or hand-roll the scope here.
- The skill assigns severity to each finding — **preserve it**.

## Step 2 — No-op when there are no Rust changes

Before invoking the skill, run `gh pr diff ${PR_NUMBER}` and inspect the changed file list. If the diff contains **no changes to `*.rs`, `Cargo.toml`, or `Cargo.lock` files**, do the following and stop:

1. Write `pass` to `./.opencode-verdict.txt` in the workspace root (e.g. `echo pass > ./.opencode-verdict.txt`).
2. Post-or-update the single summary comment (see "Posting the summary" below for the discover-then-edit pattern) with this body:
   ```
   **Rust review:** no rust-relevant changes (no Rust files modified). ✅
   <!-- opencode-verdict: pass -->
   ```
3. Stop. Do not invoke the skill.

## Protocol parameters

The shared review protocol is appended after this header. Substitute its placeholder tokens with these values:

- `<KIND>` = `Rust review`
- `<KIND_HYPHEN>` = `Rust-review`
- `<SKILL_ID>` = `rust-reviewer`
- `<SKILL_PATH>` = `.agents/skills/rust-reviewer/SKILL.md`
- `<FINDING_DESC>` = `concrete issue`
- `<SEVERITY_NOTE>` = Severity is one of `Critical`, `High`, `Medium`, `Low`.
- `<INLINE_SCOPE_LIMIT>` = `Do not post inline comments for pure style preferences the skill does not flag.`
- `<SUMMARY_HEADING>` = `**Rust review** (rust-reviewer skill, full review)`
- `<NO_FALLBACK>` = `hand-rolled Rust checks`
