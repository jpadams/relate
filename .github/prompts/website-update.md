You are updating the Relate website to keep it accurate with the latest release.

## Context

The repo has two relevant checkouts:
- `.` (current directory) — the `main` branch with the engine and CLI source
- `gh-pages/` — the website served at https://relatedb.ai/

The website is a single static page. Read `gh-pages/MAINTAINING.md` first — it
is the design contract and describes what must never change. The released
`relate` binary is installed and on `PATH` for validating examples.

## Your task

### 1. Find what changed since the last sync

Read `gh-pages/last-sync-sha.txt` for the last-processed commit SHA, then:

```
git log <last-sha>..HEAD --oneline
```

If the file is missing or the SHA is invalid, use the last 20 commits.

Also read the top (newest) section of `CHANGELOG.md` — it is the distilled,
user-facing summary of the release and the primary input.

### 2. Understand what changed

For each commit or changelog entry that is **user-visible** (new/changed/removed
CLI flags or commands, changed defaults, new Cypher features, vector/spatial
APIs, storage changes, agent-skill changes, version bumps), inspect the diff
or the referenced docs to get the facts right. Authoritative sources for facts
are listed in `gh-pages/MAINTAINING.md` (§ Facts and their sources) — when in
doubt, read `docs/api/cli/spec.md` and `README.md` on `main`, or run the
installed binary (`relate --help`).

Ignore: internal refactors, test/CI changes, performance work without a
user-visible surface, ADR bookkeeping, anything unreleased or behind a flag.
**No noise** — if a change does not alter what a developer types or sees on
this page, skip it.

### 3. Update version references

Find the release tag of this run (the latest published release). Update every
`vX.Y.Z` occurrence in `gh-pages/index.html` (hero pill, install hint, footer
meta) to it. `llms.txt` carries no version.

### 4. Make surgical updates

Edit `gh-pages/index.html` and `gh-pages/llms.txt` only where facts changed:

- Quickstart steps (install, skill, database shape, create, query)
- Hero install tabs (commands, platforms)
- Example panes (`install`, `skill`, `database`, `create`, `query`) — commands,
  flags, outputs shown
- Reference tabs (`cli`, `dot`, `output`, `cypher`, `vector`, `spatial`,
  `storage`, `clients`)
- `llms.txt` command surface / flags / APIs

Rules (from MAINTAINING.md, enforced):
- Do **not** change design, layout, CSS, tone, or copy style.
- Do **not** add sections or features that are not shipped.
- Do **not** remove working examples unless the command/flag was deleted.
- Do **not** edit pipeline-owned paths at the gh-pages root: `install.sh` and
  `releases.json` are written by the release pipeline (ADR-0068, ADR-0250)
  and `wasm/` by the build pipeline. They are not part of the site's editable
  surface — never add, edit, or delete them; a hand edit is overwritten by
  the next release.
- Do **not** add numeric TCK/CIP pass counts — the site says "full openCypher
  TCK coverage".
- A genuinely new user-facing capability MUST be covered: add it as a new
  section in the reference docs (and, if it needs a walkthrough, in the
  examples section) — a release that ships Docker, auth, or a new storage
  shape without site coverage is an incomplete sync. Match the section
  style of the existing reference tabs; keep it concise (a short intro, a
  copy-paste command, one or two facts).
- Quickstart changes are the exception: the quickstart is a curated
  narrative, not a completeness surface. If a release suggests changing
  the quickstart steps (a new step, a different order, a shape change),
  do NOT edit them — open a GitHub issue (`gh issue create --title
  "website: quickstart update for <topic>" --body <what changed and the
  suggested change>`) and link it in your summary, so a human decides.
  Adding the fact to the reference section and llms.txt is still yours.
- Preserve the four agentic invariants (MAINTAINING.md § The four agentic
  invariants) in every pane you touch: `> ` prompt, one
  `→ loading skill relate` cue, dim-sweep markup, mode-aware copy
  (`data-copy`, `paneCommands`, `panePrompts`).

### 5. Update llms.txt

`gh-pages/llms.txt` is the agent-facing mirror of the page and MUST be
updated in the same run, in lockstep with `index.html`:

- Every fact you corrected in `index.html` (flags, commands, defaults, APIs,
  storage, clients) must be reflected in the matching `llms.txt` section.
- New user-facing commands/flags get a bullet; removed ones get deleted.
- Keep the structure (Installation, Usage, Key flags, Shell dot-commands,
  Cypher, Vector search, Geospatial, Storage, Embedding and remote access,
  Links) — add sections only for genuinely new capability areas.
- No version numbers in `llms.txt`; no numeric TCK/CIP counts — "full
  openCypher TCK coverage".

### 5a. Cookbook link (ADR-0130)

The cookbook at `/cookbook/` is not part of this branch: the Pages
deploy builds it from `docs/cookbook/` at the latest release tag. Never create,
edit, or delete anything under `gh-pages/cookbook/`, and never edit recipes.

If `docs/cookbook/README.md` exists at this release's tag (`git fetch
--depth 1 origin tag <tag>`, then `git show <tag>:docs/cookbook/README.md`),
the site must link it, exactly as
`gh-pages/MAINTAINING.md` (§ Cookbook) specifies. Add whatever is missing:

- the `Cookbook` item in `#topNav`
  (`<a href="/cookbook/">Cookbook</a>`), immediately before
  `GitHub` (`Playground` stays last);
- the `## Cookbook` section in `llms.txt`, linking
  `https://relatedb.ai/cookbook/` and
  `https://relatedb.ai/cookbook/llms.txt`.

If the tag has no `docs/cookbook/`, add neither: the route would 404.

### 6. Validate the examples

Extract every `relate …` command shown on the page (`<pre>` blocks and
`data-copy` / `paneCommands` values) and run the ones that are safe:

- `--help`, `--version`, `upgrade --check`, `telemetry status`: run directly.
- One-shot queries against `:memory:` or a temp directory (e.g.
  `relate :memory: 'RETURN 1'`, create + query in `/tmp`): run them and
  check exit codes and that the output shape matches what the pane shows.
- Commands needing real external state (none should, but if any appear):
  skip and note them as untested.

If a command fails because the CLI changed, fix the example on the page — the
binary is the source of truth.

### 7. Update the sync marker

```
git rev-parse HEAD > gh-pages/last-sync-sha.txt
```

### 8. Print a summary

End your response with a brief summary of what you changed (or "No changes
needed"), preceded by this exact marker line (underscore, no spaces — the
workflow greps for it to confirm you ran to completion):

```
<!-- SUMMARY_START -->
```

List: version updates made, facts corrected, examples validated (pass/fixed)
and skipped.
