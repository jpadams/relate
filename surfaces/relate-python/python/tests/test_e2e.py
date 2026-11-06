"""End-to-end tests for the module API and full database lifecycle."""

import relate


# ============================================================
# Module-level API
# ============================================================


def test_connect_returns_connection():
    db = relate.connect(":memory:")
    assert isinstance(db, relate.Connection)
    db.close()


def test_version_attribute():
    assert hasattr(relate, "__version__")
    assert isinstance(relate.__version__, str)
    assert len(relate.__version__) > 0


def test_module_exports():
    """All public names in __all__ should be importable."""
    for name in relate.__all__:
        assert hasattr(relate, name), f"Missing export: {name}"


# ============================================================
# Persistent database E2E
# ============================================================


def test_full_lifecycle_persistent(tmp_path):
    """Full create -> close -> reopen cycle with a Relate directory."""
    path = str(tmp_path / "lifecycle")

    # Create data
    with relate.connect(path) as db:
        db.execute("CREATE (:City {name: 'Portland'})")
        db.execute("CREATE (:City {name: 'Seattle'})")
        cursor = db.execute("MATCH (c:City) RETURN count(*) AS c")
        assert cursor.fetchone()["c"] == 2

    # Reopen and verify
    with relate.connect(path) as db:
        cursor = db.execute(
            "MATCH (c:City) RETURN c.name ORDER BY c.name"
        )
        names = [r["c.name"] for r in cursor]
        assert names == ["Portland", "Seattle"]

        # Add more data
        db.execute("CREATE (:City {name: 'San Francisco'})")

    # Verify accumulated data
    with relate.connect(path) as db:
        cursor = db.execute("MATCH (c:City) RETURN count(*) AS c")
        assert cursor.fetchone()["c"] == 3


# ============================================================
# Complex query patterns
# ============================================================


def test_graph_traversal(db):
    """Multi-hop graph traversal should work through the Python bindings."""
    db.execute("CREATE (:Person {name: 'A'})")
    db.execute("CREATE (:Person {name: 'B'})")
    db.execute("CREATE (:Person {name: 'C'})")
    db.execute(
        "MATCH (a:Person {name: 'A'}), (b:Person {name: 'B'}) "
        "CREATE (a)-[:KNOWS]->(b)"
    )
    db.execute(
        "MATCH (b:Person {name: 'B'}), (c:Person {name: 'C'}) "
        "CREATE (b)-[:KNOWS]->(c)"
    )

    # 2-hop traversal
    cursor = db.execute(
        "MATCH (a:Person {name: 'A'})-[:KNOWS]->(mid)-[:KNOWS]->(dest) "
        "RETURN dest.name"
    )
    assert cursor.fetchone()["dest.name"] == "C"


def test_merge_operation(db):
    """MERGE should create or match."""
    db.execute("MERGE (p:Person {name: 'Alice'})")
    db.execute("MERGE (p:Person {name: 'Alice'})")  # should not duplicate
    cursor = db.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 1


def test_with_clause(db):
    """WITH clause for query chaining."""
    db.executemany("CREATE (:N {v: $v})", [{"v": i} for i in range(5)])
    cursor = db.execute(
        "MATCH (n:N) "
        "WITH n.v AS val "
        "WHERE val > 2 "
        "RETURN val ORDER BY val"
    )
    assert [r["val"] for r in cursor] == [3, 4]


def test_unwind(db):
    """UNWIND should expand a list into rows."""
    cursor = db.execute("UNWIND [1, 2, 3] AS x RETURN x")
    assert [r["x"] for r in cursor] == [1, 2, 3]


def test_optional_match(db):
    """OPTIONAL MATCH should return null for missing patterns."""
    db.execute("CREATE (:Person {name: 'Alice'})")
    cursor = db.execute(
        "MATCH (p:Person {name: 'Alice'}) "
        "OPTIONAL MATCH (p)-[:KNOWS]->(friend) "
        "RETURN p.name, friend.name AS fname"
    )
    row = cursor.fetchone()
    assert row["p.name"] == "Alice"
    assert row["fname"] is None


# ============================================================
# Many rapid connections
# ============================================================


def test_rapid_open_close():
    """Opening and closing many connections rapidly should not leak."""
    for _ in range(10):
        db = relate.connect(":memory:")
        db.execute("RETURN 1 AS x")
        db.close()
