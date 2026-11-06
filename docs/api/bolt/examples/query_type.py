#!/usr/bin/env python3
"""Read the result summary's query type and counters over Bolt with the
official Neo4j driver (ADR-0162, ADR-0309).

Start the server first — behind the `--bolt-identity` compat shim,
because the current python driver validates the HELLO `server` agent and
rejects anything not starting `Neo4j/` (ADR-0293):
  cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --bolt-identity Neo4j/5.26.0
The port comes from RELATE_BOLT_PORT_IDENTITY (scripts/bolt-smoke.sh
exports it beside the spawn; unset is a configuration error).
"""
from _topology import bolt_uri
from neo4j import GraphDatabase

def main():
    driver = GraphDatabase.driver(
        bolt_uri("RELATE_BOLT_PORT_IDENTITY"), auth=("neo4j", "unused"))
    # The driver surfaces the ending SUCCESS `type` key (LABS-323 findings
    # F-13/F-40 / D-52..D-56) as `summary.query_type` and its `stats` map
    # as `summary.counters`.
    _, summary, _ = driver.execute_query("MATCH (n) RETURN n")
    assert summary.query_type == "r", summary.query_type  # F-13 / D-55
    # A private label keeps the example independent of sibling state: CI runs
    # the doc examples sequentially against one shared :memory: server.
    _, summary, _ = driver.execute_query("CREATE (n:QueryTypeProbe)")
    assert summary.query_type == "w", summary.query_type  # F-13 / D-52
    assert summary.counters.nodes_created == 1, summary.counters
    assert summary.counters.labels_added == 1, summary.counters
    _, summary, _ = driver.execute_query(
        "MATCH (n:QueryTypeProbe) SET n.seen = true RETURN n"
    )
    assert summary.query_type == "rw", summary.query_type  # D-53
    # D-53/ADR-0332: a write with no returned columns is "w" — a MATCH that
    # only feeds a SET returns no columns.
    _, summary, _ = driver.execute_query(
        "MATCH (n:QueryTypeProbe) SET n.seen = false"
    )
    assert summary.query_type == "w", summary.query_type  # D-53
    # D-54: a DETACH DELETE that returns no columns is "w" too.
    driver.execute_query("CREATE (n:QueryTypeDeleteMe)")
    _, summary, _ = driver.execute_query(
        "MATCH (n:QueryTypeDeleteMe) DETACH DELETE n"
    )
    assert summary.query_type == "w", summary.query_type  # D-54
    assert summary.counters.nodes_deleted == 1, summary.counters
    _, summary, _ = driver.execute_query(
        "CREATE INDEX query_type_probe_name FOR (n:QueryTypeProbe) ON (n.name)"
    )
    assert summary.query_type == "s", summary.query_type  # D-56
    assert summary.counters.indexes_added == 1, summary.counters
    # EXPLAIN reports the wrapped statement's type and executes nothing.
    _, summary, _ = driver.execute_query("EXPLAIN CREATE (n:QueryTypeProbe)")
    assert summary.query_type == "w", summary.query_type
    records, _, _ = driver.execute_query(
        "MATCH (n:QueryTypeProbe) RETURN count(n) AS c"
    )
    assert records[0]["c"] == 1  # only the CREATE above ran
    driver.close()
    print("ok")

if __name__ == "__main__":
    main()
