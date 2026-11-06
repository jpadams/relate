"""The structured plan document (LABS-418, ADR-0381).

The native ``QueryResult.plan`` and the DB-API ``ResultSummary.plan`` carry
the ``relate.plan/v1`` document for a statement that planned a tree
(``EXPLAIN``/``PROFILE``) and ``None`` for a plain run. Each node's
``warnings`` carry the engine's remedy-bearing notifications (ADR-0363),
joined by operator id — the diagnosis *and* the action.
"""

import relate

SCHEMA = "relate.plan/v1"
CARTESIAN_CODE = "Neo.ClientNotification.Statement.CartesianProduct"


def _walk(node):
    """Every node of the document's tree, pre-order."""
    yield node
    for child in node["children"]:
        yield from _walk(child)


def test_plain_run_carries_no_plan(db, native_db):
    """A statement with no plan document reports ``None`` on both paths."""
    assert db.execute("RETURN 1 AS n").consume().plan is None
    assert native_db.execute("RETURN 1 AS n").plan is None


def test_explain_carries_the_document(native_db):
    doc = native_db.execute("EXPLAIN MATCH (p:Person) RETURN p.name AS name").plan
    assert doc["schema"] == SCHEMA
    assert doc["profiled"] is False
    assert doc["statement"] == "MATCH (p:Person) RETURN p.name AS name"
    root = doc["root"]
    assert root["operator"] == "ProduceResults"
    assert root["actual"] is None, "EXPLAIN carries no runtime metrics"
    # The tree is nested: the label scan rides beneath the projection.
    assert "NodeByLabelScan" in [n["operator"] for n in _walk(root)]


def test_profile_carries_metrics_and_the_walk_start(native_db):
    native_db.execute("CREATE (:Person {name: 'Alice'})")
    native_db.execute("CREATE (:Person {name: 'Bob'})")

    doc = native_db.execute("PROFILE MATCH (p:Person) RETURN p.name AS name").plan
    assert doc["profiled"] is True
    scan = next(n for n in _walk(doc["root"]) if n["operator"] == "NodeByLabelScan")
    actual = scan["actual"]
    assert actual["rows"] == 2
    assert actual["node_reads"] == 2, "one store read per candidate examined"
    assert actual["start"]["reason"] == "index-0"
    assert actual["start"]["backward"] is False


def test_a_node_warning_carries_its_remedy(native_db):
    native_db.execute("CREATE (:Person)")
    native_db.execute("CREATE (:City)")

    doc = native_db.execute("EXPLAIN MATCH (a:Person), (b:City) RETURN a, b").plan
    warnings = [w for n in _walk(doc["root"]) for w in n["warnings"]]
    assert warnings, "the cartesian MATCH warns on its operator"
    entry = warnings[0]
    assert entry["code"] == CARTESIAN_CODE
    assert entry["severity"] == "INFORMATION"
    assert "join anchor" in entry["remedy"], "the warning carries the action"


def test_the_summary_matches_the_native_result(db, native_db):
    db.execute("CREATE (:Person)")
    native_db.execute("CREATE (:Person)")
    native_doc = native_db.execute("EXPLAIN MATCH (p:Person) RETURN p").plan
    summary_doc = db.execute("EXPLAIN MATCH (p:Person) RETURN p").consume().plan
    assert summary_doc == native_doc, "both paths project the same document"
