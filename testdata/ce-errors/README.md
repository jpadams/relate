# Neo4j Community Edition error fixtures

`neo4j-ce.json` pins the exact error messages, GQLSTATUS codes and legacy
Neo4j status codes that Neo4j Community Edition (branch `2026.09`) produces
for the database-administration and graph-routing statements that
ADR-0116/ADR-0117 model in Relate. Engine tests assert against this
fixture; the CE text is authoritative, so divergences from the ADR wording
are recorded (per case, in `note` fields, and in
`scripts/ce_error_fixtures.py`), never edited away.

The committed fixture was captured against a live `neo4j:2026.09-community`
container (`source: "live-container"`) by
[`scripts/ce_error_fixtures.py`](../../scripts/ce_error_fixtures.py); the
script pins each case's root-cause GQLSTATUS from the neo4j/neo4j branch
`2026.09` CE acceptance-test/GQLSTATUS sources, so every case carries a
`source_url` even though the messages are the live server text.

## Regenerating against a live CE server

See the header docstring of [`scripts/ce_error_fixtures.py`](../../scripts/ce_error_fixtures.py)
for the full usage, including the `docker run` commands (the read-only case
needs a second container started with
`-e NEO4J_server_databases_read__only=neo4j`). To just validate the committed
fixture:

```sh
python3 scripts/ce_error_fixtures.py --fixtures-only
```

## Fixture shape

- Top level: `neo4j_version`, `source` (`live-container` or
  `acceptance-tests`), `captured_at` (UTC), `cases`, `show_databases`.
- Each case: `id`, `statement`, `database` (session database),
  `gqlstatus` (diagnostic code the ADRs reference), `neo4j_code`
  (legacy `Neo.ClientError.*` code), `message`, plus optional
  `gqlstatus_wire` (top-level GQLSTATUS seen by drivers), `outcome`
  (`error`/`success`), `status_description`, `note`, `source_url`.
- `show_databases`: CE `SHOW DATABASES` rows for `neo4j` and `system` with
  `name`, `type`, `access`, `currentStatus`, `requestedStatus`, `default`,
  `home`, `aliases`, `constituents`.
