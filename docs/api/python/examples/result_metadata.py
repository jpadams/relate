"""QueryResult metadata: resolved namespace, query type and notifications.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/result_metadata.py
"""

import relate


def main():
    # The native Database.execute() returns a QueryResult carrying the
    # statement's resolved namespace and plan-time query type.
    db = relate.Database(":memory:")

    # EXPLAIN reports the wrapped statement's query type without executing
    # it: the CREATE never runs, so the graph stays empty.
    explained = db.execute("EXPLAIN CREATE (n)")
    assert explained.query_type == "w"
    assert explained.namespace == "neo4j"
    assert db.execute("MATCH (n) RETURN n").data == []

    # Query type value set (ADR-0162, ADR-0332): "r" read, "w" a write with
    # no returned columns, "rw" a write with returned columns, "s" schema.
    assert db.execute("MATCH (n) RETURN n").query_type == "r"
    assert db.execute("CREATE (n)").query_type == "w"

    # A USE-routed statement reports its resolved namespace; the session
    # namespace is unchanged, so a following unrouted statement still
    # resolves to "neo4j".
    routed = db.execute("USE system SHOW DATABASES YIELD name RETURN name")
    assert routed.namespace == "system"
    assert db.execute("RETURN 1 AS n").namespace == "neo4j"

    # elapsed_ms is the engine-reported wall-clock milliseconds for the
    # statement (ADR-0221) — parse, plan and run, not client-side handling.
    # Its phase split (ADR-0491) breaks the total into the PARSE / PLAN /
    # EXECUTE windows: disjoint sub-windows whose sum never exceeds it.
    timed = db.execute("RETURN 1 AS n")
    assert timed.elapsed_ms >= 0.0
    assert timed.parse_ms >= 0.0
    assert timed.plan_ms >= 0.0
    assert timed.execute_ms >= 0.0
    assert timed.parse_ms + timed.plan_ms + timed.execute_ms <= timed.elapsed_ms

    # column_types is the engine-reported per-column type (ADR-0224),
    # stamped from the first row; a result with no rows stamps ANY.
    typed = db.execute("RETURN 1 AS i, 'hi' AS s")
    assert typed.column_types == ["INTEGER", "STRING"]
    empty = db.execute("MATCH (n:Missing) RETURN n.name AS name")
    assert empty.column_types == ["ANY"]

    # notifications is the engine-native query-notification list (ADR-0363),
    # derived only where the plan tree is built: a plain run carries none.
    assert timed.notifications == []
    db.execute("CREATE (:Person)")
    db.execute("CREATE (:City)")
    cartesian = db.execute("EXPLAIN MATCH (a:Person), (b:City) RETURN a, b")
    # Each entry carries code, gql_status, severity, category, title,
    # description, remedy and — on operator-scoped kinds — operator_id.
    [notification] = cartesian.notifications
    assert (
        notification["code"]
        == "Neo.ClientNotification.Statement.CartesianProduct"
    )
    assert notification["gql_status"] == "03N90"
    assert notification["severity"] == "INFORMATION"
    assert notification["category"] == "PERFORMANCE"
    assert notification["remedy"]
    assert "operator_id" in notification

    db.close()
    print("result_metadata example OK")


if __name__ == "__main__":
    main()
