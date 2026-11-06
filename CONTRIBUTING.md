# Contributing to Relate

Thanks for using Relate — and for wanting to make it better.

## Issues only — no pull requests

Relate does **not** accept pull requests. All development and review happens
with the maintainers; this repository is the public home of the source, the
docs, and the releases. An opened pull request will be closed with a pointer
back to this file — not because the work is unwelcome, but because the team
cannot give external code the review bandwidth it deserves.

What we **do** accept, and read carefully, are
[issues](https://github.com/relatedb/relate/issues):

- **Bugs** — something behaves wrong, errors badly, or crashes.
- **Feature requests** — a use case the engine, CLI, or a client doesn't cover.
- **Questions** — when the docs don't get you there (we'll fix the docs).

## Filing a good bug report

1. **Version and platform** — the `relate --version` output, OS, arch.
2. **A minimal reproduction** — the smallest Cypher statement or CLI
   invocation that shows the problem, plus what you expected instead.
3. **For storage issues** — how the store was opened: a directory, a `.rl`
   file, or `:memory:`; snapshot format and durability mode if you changed
   them from the defaults.

For Python, WASM, or client-library issues, add the package version and how
it was installed (a registry wheel or built from source).

## Filing a good feature request

Lead with the use case, not the solution: what you're trying to accomplish,
what you tried, and where it fell short. Coverage is broad — check the
[cookbook](docs/cookbook/) and the [references](docs/reference/) first; the
answer may already exist.

## Where to read before filing

- [README](README.md) — install and quick start
- [`docs/guides/`](docs/guides/) — task guides: vector similarity, geospatial,
  CSV import, backup/restore, the storage engine
- [`docs/reference/`](docs/reference/) — Cypher syntax, functions, CLI flags,
  storage formats, the C ABI
- [`docs/adr/`](docs/adr/) — why things are the way they are

Thanks for helping make Relate better.
