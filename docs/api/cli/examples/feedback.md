# Example: consent to and send a feedback report

A feedback report is person-consented. A human grants the authorization once;
it lasts 30 days.

```bash
relate feedback enable
```

Expected stdout is `feedback enabled until <RFC3339> (<path to config.json>)`.
Then report one friction point — what you expected and what actually happened:

```bash
relate feedback submit \
  --category unclear_error \
  --expected "the info command to print the version" \
  --observed "it printed help instead" \
  --command info
```

Expected stdout is one compact JSON outcome document, e.g.
`{"outcome":"sent","reason":"sent","request_id":"…","report_id":"…",…}`.
Exit code 0 means the report was sent or skipped (feedback not enabled,
telemetry opted out); 1 means it was rejected locally or the send failed; 2
is a usage error. `--expected` and `--observed` are free text, so they must
not contain emails, URLs, host names, IP addresses, file paths, data
filenames, secrets, Cypher, or pasted output — a report containing any of
those is rejected whole and never sent. The one exception: a documentation
link to this project's own docs or repository, by path (`relatedb.ai/<path>`,
`github.com/relatedb/relate/<path>`), or to the Neo4j Cypher manual
(`neo4j.com/docs/`) is kept; a bare host URL with no path (`relatedb.ai`,
`github.com/relatedb/relate`) is refused.

Check the state, then revoke the authorization:

```bash
relate feedback status     # enabled (until <RFC3339>) or disabled (<reason>)
relate feedback disable
```

Expected `disable` stdout is `feedback disabled (<path to config.json>)`.
Later submits send nothing and print an outcome with `"reason":"not_enabled"`.

See the [CLI spec](../spec.md#feedback-reports) for the full flag table, exit
codes, and environment overrides.
