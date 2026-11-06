"""The structured plan document: estimates, PROFILE metrics and remedies.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/plan.py
"""

import relate

SCHEMA = "relate.plan/v1"


def walk(node):
    """Every operator of the plan tree, pre-order."""
    yield node
    for child in node["children"]:
        yield from walk(child)


def main():
    db = relate.Database(":memory:")
    db.execute("CREATE (:Person {name: 'Alice'})-[:KNOWS]->(:Person {name: 'Bob'})")
    db.execute("CREATE (:City {name: 'Berlin'})")

    # EXPLAIN stamps the tree with no runtime metrics: the document says
    # profiled=False and every node's `actual` is None.
    explained = db.execute("EXPLAIN MATCH (p:Person) RETURN p.name AS name").plan
    assert explained["schema"] == SCHEMA
    assert explained["profiled"] is False
    assert explained["statement"].startswith("MATCH (p:Person)")

    # The tree is nested: the label scan rides beneath the projection, and
    # it names its estimate (the AllNodesScan-style warning machinery reads
    # the same fact).
    scan = next(n for n in walk(explained["root"]) if n["operator"] == "NodeByLabelScan")
    assert scan["details"].startswith("(p:Person)")
    assert scan["actual"] is None, "EXPLAIN carries no metrics"

    # PROFILE carries the metrics: rows, db hits, the read-kind mix and the
    # walk's chosen start — the facts an opaque "Expand" used to hide.
    profiled = db.execute(
        "PROFILE MATCH (p:Person)-[k:KNOWS]->(f) RETURN p.name AS name"
    ).plan
    assert profiled["profiled"] is True
    expand = next(n for n in walk(profiled["root"]) if n["operator"] == "Expand")
    actual = expand["actual"]
    assert actual["rows"] == 1
    assert actual["neighbor_walks"] >= 1
    assert actual["start"]["reason"] in {"bound-pivot", "cheapest", "seek-anchored", "index-0"}

    # A warning carries the action, not just the diagnosis (ADR-0363): the
    # disconnected MATCH raises the cartesian notification on its operator,
    # with the remedy text an agent can act on.
    warned = db.execute("EXPLAIN MATCH (a:Person), (c:City) RETURN a, c").plan
    warnings = [w for n in walk(warned["root"]) for w in n["warnings"]]
    assert warnings, "the cartesian MATCH warns"
    assert warnings[0]["code"] == "Neo.ClientNotification.Statement.CartesianProduct"
    assert warnings[0]["remedy"], "every warning carries its remedy"

    # A plain run carries no plan: the document is None, never an empty one.
    assert db.execute("RETURN 1 AS n").plan is None

    # The DB-API summary carries the same document (the Cursor's consume()).
    conn = relate.connect(":memory:")
    summary = conn.execute("EXPLAIN MATCH (p:Person) RETURN p").consume()
    assert summary.plan["schema"] == SCHEMA
    conn.close()

    db.close()
    print("plan example ok")


if __name__ == "__main__":
    main()
