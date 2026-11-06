# Example: opt in for Bolt clients

> **Retired start command (ADR-0368).** Server-side `LOAD CSV` over Bolt
> was reached through the standalone `relate-bolt` binary
> (`--load-csv-dir`), which is retired (LABS-381). The capability is
> **library-level only** now — no product binary exposes it; embed
> `relate_bolt::serve` with a `--load-csv-dir` argument to serve a
> store with a LOAD CSV root. The protocol and behavior below are
> unchanged.

An operator opts a serving process into bounded, rooted file reads by
setting a CSV root: `--load-csv-dir "$csv_dir"` on the library `serve(&Args)`
entry point, or `RELATE_LOAD_CSV_DIR` in the environment (the flag
wins); client queries cannot choose a new root. A scratch root for the
values this example checks:

```bash
csv_dir=$(mktemp -d)
printf 'Ada\nLin\n' > "$csv_dir/people.csv"
python3 - "$csv_dir/too_large.csv" <<'PY'
import sys
with open(sys.argv[1], "wb") as source:
    source.write(b"x" * (8_388_608 + 1))
PY
# After the run: rm -rf "$csv_dir"
```

With the official Neo4j Python driver installed
(`python3 -m pip install neo4j`), the [driver example](load_csv.py) runs
against a server whose `--load-csv-dir` is the scratch root above; it is
run by the crate's own test harness in-process (ADR-0368 §3), not as a
documented product-binary command pair:

```bash
python3 docs/api/bolt/examples/load_csv.py
```

The example verifies the two imported names and that a file one byte over
the default 8 MiB (8,388,608-byte) cap returns a generic error without
exposing its path. Without the root or `RELATE_LOAD_CSV_DIR`, local
`LOAD CSV` fails with `LOAD CSV file unavailable`; each source is bounded
to 8 MiB by default.
