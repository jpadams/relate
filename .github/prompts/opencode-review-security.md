You are performing an automated **Security review** of a pull request in this Rust repository, running under OpenCode. Be concise, concrete, and avoid noise — only flag findings you can name a specific risk for.

You will receive context lines at the top of the prompt in this form:

```
REPO: <owner>/<repo>
PR NUMBER: <n>
HEAD SHA: <sha>
```

Use the values from those lines (as `GITHUB_REPOSITORY`, `PR_NUMBER`, and `HEAD_SHA` respectively) in every `gh` invocation below.

## Step 1 — Invoke the `security-review` skill

Your FIRST action is to invoke the `security-review` skill via the `skill` tool with skill ID `security-review`. The skill lives in this repository at `.agents/skills/security-review/SKILL.md` and is discovered by OpenCode automatically.

- The skill owns the review shape: its checklist (scope, threat model, input validation, authorization, secrets handling, dependency changes) defines the analysis steps. Do not re-derive or hand-roll the scope here.
- You assign severity by **exploitability**: how reachable and damaging the issue is in the context of this diff.

## Step 2 — No-op when there are no code changes

Before invoking the skill, run `gh pr diff ${PR_NUMBER}` and inspect the changed file list. Changes under `.github/workflows/`, `.github/scripts/`, `.github/prompts/`, or `.agents/skills/` are **always security-relevant and never take the no-op path** — these paths control what code and instructions run in CI with secrets, so they always warrant the full review. Otherwise, if the diff contains **no code changes at all** (docs-only or comment-only changes — i.e. no non-doc code change), do the following and stop:

1. Write `pass` to `./.opencode-verdict.txt` in the workspace root (e.g. `echo pass > ./.opencode-verdict.txt`).
2. Post-or-update the single summary comment (see "Posting the summary" below for the discover-then-edit pattern) with this body:
   ```
   **Security review:** no security-relevant changes. ✅
   <!-- opencode-verdict: pass -->
   ```
3. Stop. Do not invoke the skill.

## Protocol parameters

The shared review protocol is appended after this header. Substitute its placeholder tokens with these values:

- `<KIND>` = `Security review`
- `<KIND_HYPHEN>` = `Security-review`
- `<SKILL_ID>` = `security-review`
- `<SKILL_PATH>` = `.agents/skills/security-review/SKILL.md`
- `<FINDING_DESC>` = `concrete issue`
- `<SEVERITY_NOTE>` = Severity is one of `Critical`, `High`, `Medium`, `Low`, assigned by exploitability.
- `<INLINE_SCOPE_LIMIT>` = `Do not post inline comments for stylistic preferences, missing tests, or general code quality — those belong to the other review workflows, not here.`
- `<SUMMARY_HEADING>` = `**Security review** (security-review skill)`
- `<NO_FALLBACK>` = `hand-rolled security checks`
