# Example: run a database from a URL

Point `--database` at a published `.rl` bundle — no download step. The
local copy opens writable by default; `--read-only` makes it a read-only
scratch copy:

```bash
cargo run -p relate-cli -- \
  -d https://github.com/relatedb/relate/blob/main/test.rl \
  'MATCH (n) RETURN count(n)'
```

A GitHub **blob** link is the shareable form and is rewritten to its
`raw.githubusercontent.com` bytes behind the scenes — as is GitHub's own
**raw** verb (`.../raw/<ref>/path.rl`), which would otherwise redirect
there. A raw URL, a release-asset URL, and any other link that redirects to
the bytes work as they are: redirects are followed, and an `https://` fetch
refuses a hop that would downgrade to plain `http://`. The fetch is
announced on stderr; a writable copy is kept at the announced path (its
writes are the user's), a `--read-only` copy is scratch and is removed on
exit — and a write on a read-only copy refuses with
`WriteOnReadOnlyAccessDatabase` (exit 6). `--read-only=live` is a usage
error (exit 2):

```bash
# write refused: this copy was opened read-only on purpose
cargo run -p relate-cli -- --read-only \
  -d https://raw.githubusercontent.com/relatedb/relate/main/test.rl \
  'CREATE (n:Fresh)'
```

Policy refusals happen before any network I/O: `--safe` refuses a remote
`--database` URL outright (exit 2), and so do a repository/`tree` URL, a
URL that is not a `.rl` file, and an empty URL — each naming the form that
works. A non-2xx response is a runtime error (exit 1) reporting
`HTTP <status>`, and the body is capped at 1 GiB with a 300 s timeout.

Without the `remote-database` feature the URL still parses and the fetch
fails closed (exit 1) with `--database URL is not compiled into this build
(enable the "remote-database" feature)`; the `full` and `agent` bundles
carry it (ADR-0496).

The e2e suite `e2e_run_from_url.rs` executes these shapes against the
binary: it clones a real `.rl` bundle and serves it from a loopback stub,
then pins the read-only query, the write refusal, both policy refusals, the
HTTP-error class, and the usage errors. The bundle suite pins the
fails-closed message for builds without the feature.
