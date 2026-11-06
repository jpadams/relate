"""Integration tests for Relate Python bindings (requires built native module)."""

import pytest

import relate


# ============================================================
# Basic data type round-trips
# ============================================================


def test_return_literal(db):
    cursor = db.execute("RETURN 42 AS answer")
    row = cursor.fetchone()
    assert row is not None
    assert row["answer"] == 42


def test_return_string(db):
    cursor = db.execute("RETURN 'hello' AS greeting")
    assert cursor.fetchone()["greeting"] == "hello"


def test_return_boolean(db):
    cursor = db.execute("RETURN true AS flag")
    assert cursor.fetchone()["flag"] is True


def test_return_boolean_false(db):
    cursor = db.execute("RETURN false AS flag")
    assert cursor.fetchone()["flag"] is False


def test_return_null(db):
    cursor = db.execute("RETURN null AS nothing")
    assert cursor.fetchone()["nothing"] is None


def test_return_list(db):
    cursor = db.execute("RETURN [1, 2, 3] AS nums")
    assert cursor.fetchone()["nums"] == [1, 2, 3]


def test_return_float(db):
    cursor = db.execute("RETURN 3.14 AS pi")
    assert abs(cursor.fetchone()["pi"] - 3.14) < 0.001


def test_return_negative_number(db):
    cursor = db.execute("RETURN -42 AS neg")
    assert cursor.fetchone()["neg"] == -42


def test_return_zero(db):
    cursor = db.execute("RETURN 0 AS z")
    assert cursor.fetchone()["z"] == 0


def test_return_empty_string(db):
    cursor = db.execute("RETURN '' AS empty")
    assert cursor.fetchone()["empty"] == ""


def test_return_list_via_range(db):
    """Lists can be created via range()."""
    cursor = db.execute("RETURN range(1, 3) AS nums")
    assert cursor.fetchone()["nums"] == [1, 2, 3]


def test_return_multiple_columns(db):
    cursor = db.execute("RETURN 1 AS a, 'two' AS b, true AS c, null AS d")
    row = cursor.fetchone()
    assert row == {"a": 1, "b": "two", "c": True, "d": None}


# ============================================================
# Node and relationship operations
# ============================================================


def test_create_and_match(db):
    db.execute("CREATE (:Person {name: 'Alice', age: 30})")
    cursor = db.execute("MATCH (p:Person) RETURN p.name, p.age")
    row = cursor.fetchone()
    assert row["p.name"] == "Alice"
    assert row["p.age"] == 30


def test_create_relationship(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.execute("CREATE (:Person {name: 'Bob'})")
    db.execute(
        "MATCH (a:Person {name: 'Alice'}), (b:Person {name: 'Bob'}) "
        "CREATE (a)-[:KNOWS]->(b)"
    )
    cursor = db.execute(
        "MATCH (a:Person)-[:KNOWS]->(b:Person) "
        "RETURN a.name, b.name"
    )
    row = cursor.fetchone()
    assert row["a.name"] == "Alice"
    assert row["b.name"] == "Bob"


def test_relationship_with_properties(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.execute("CREATE (:Person {name: 'Bob'})")
    db.execute(
        "MATCH (a:Person {name: 'Alice'}), (b:Person {name: 'Bob'}) "
        "CREATE (a)-[:FRIENDS_WITH {since: 2020, strength: 0.9}]->(b)"
    )
    cursor = db.execute(
        "MATCH (a:Person)-[r:FRIENDS_WITH]->(b:Person) "
        "RETURN r.since, r.strength"
    )
    row = cursor.fetchone()
    assert row["r.since"] == 2020
    assert abs(row["r.strength"] - 0.9) < 0.001


def test_multiple_labels(db):
    """Nodes can have multiple labels."""
    db.execute("CREATE (:Person:Employee {name: 'Alice'})")
    cursor = db.execute("MATCH (p:Person:Employee) RETURN p.name")
    assert cursor.fetchone()["p.name"] == "Alice"


def test_set_property(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.execute("MATCH (p:Person {name: 'Alice'}) SET p.age = 30")
    cursor = db.execute("MATCH (p:Person {name: 'Alice'}) RETURN p.age")
    assert cursor.fetchone()["p.age"] == 30


def test_remove_property(db):
    db.execute("CREATE (:Person {name: 'Alice', age: 30})")
    db.execute("MATCH (p:Person {name: 'Alice'}) REMOVE p.age")
    cursor = db.execute("MATCH (p:Person {name: 'Alice'}) RETURN p.age")
    assert cursor.fetchone()["p.age"] is None


def test_detach_delete(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.execute("CREATE (:Person {name: 'Bob'})")
    db.execute(
        "MATCH (a:Person {name: 'Alice'}), (b:Person {name: 'Bob'}) "
        "CREATE (a)-[:KNOWS]->(b)"
    )
    db.execute("MATCH (n:Person) DETACH DELETE n")
    cursor = db.execute("MATCH (n) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 0


# ============================================================
# Parameters
# ============================================================


def test_parameters(db):
    db.execute(
        "CREATE (:Person {name: $name, age: $age})",
        {"name": "Bob", "age": 25},
    )
    cursor = db.execute(
        "MATCH (p:Person {name: $name}) RETURN p.age",
        {"name": "Bob"},
    )
    assert cursor.fetchone()["p.age"] == 25


def test_parameters_string_with_special_chars(db):
    db.execute(
        "CREATE (:Person {name: $name})",
        {"name": "O'Brien"},
    )
    cursor = db.execute("MATCH (p:Person) RETURN p.name")
    assert cursor.fetchone()["p.name"] == "O'Brien"


def test_parameters_empty_dict(db):
    """Empty parameter dict should work the same as None."""
    cursor = db.execute("RETURN 1 AS x", {})
    assert cursor.fetchone()["x"] == 1


def test_parameters_no_params(db):
    """Omitting params entirely should work."""
    cursor = db.execute("RETURN 1 AS x")
    assert cursor.fetchone()["x"] == 1


# ============================================================
# Mutation stats
# ============================================================


def test_stats_on_create(db):
    cursor = db.execute("CREATE (:Foo {x: 1, y: 2})")
    assert cursor.stats["nodes_created"] == 1
    assert cursor.stats["properties_set"] == 2


def test_stats_on_delete(db):
    db.execute("CREATE (:Temp)")
    cursor = db.execute("MATCH (t:Temp) DELETE t")
    assert cursor.stats["nodes_deleted"] == 1


def test_stats_on_relationship_create(db):
    db.execute("CREATE (:A {name: 'a'})")
    db.execute("CREATE (:B {name: 'b'})")
    cursor = db.execute("MATCH (a:A), (b:B) CREATE (a)-[:REL]->(b)")
    assert cursor.stats["relationships_created"] == 1


def test_stats_keys_present(db):
    """All expected stat keys should be present."""
    cursor = db.execute("CREATE (:Test)")
    stats = cursor.stats
    expected_keys = {
        "nodes_created",
        "nodes_deleted",
        "relationships_created",
        "relationships_deleted",
        "properties_set",
        "labels_added",
        "labels_removed",
    }
    for key in expected_keys:
        assert key in stats, f"Missing stat key: {key}"


# ============================================================
# Multiple rows and ordering
# ============================================================


def test_multiple_rows(db):
    db.execute("CREATE (:N {v: 1})")
    db.execute("CREATE (:N {v: 2})")
    db.execute("CREATE (:N {v: 3})")
    cursor = db.execute("MATCH (n:N) RETURN n.v ORDER BY n.v")
    rows = cursor.fetchall()
    assert len(rows) == 3
    assert [r["n.v"] for r in rows] == [1, 2, 3]


def test_order_by_desc(db):
    db.execute("CREATE (:N {v: 1})")
    db.execute("CREATE (:N {v: 2})")
    db.execute("CREATE (:N {v: 3})")
    cursor = db.execute("MATCH (n:N) RETURN n.v ORDER BY n.v DESC")
    assert [r["n.v"] for r in cursor] == [3, 2, 1]


def test_limit(db):
    for i in range(5):
        db.execute(f"CREATE (:N {{v: {i}}})")
    cursor = db.execute("MATCH (n:N) RETURN n.v ORDER BY n.v LIMIT 3")
    assert len(cursor.fetchall()) == 3


def test_skip_and_limit(db):
    for i in range(5):
        db.execute(f"CREATE (:N {{v: {i}}})")
    cursor = db.execute("MATCH (n:N) RETURN n.v ORDER BY n.v SKIP 2 LIMIT 2")
    assert [r["n.v"] for r in cursor] == [2, 3]


# ============================================================
# Empty and edge-case results
# ============================================================


def test_empty_result(db):
    cursor = db.execute("MATCH (n:Nothing) RETURN n")
    assert cursor.fetchall() == []
    assert cursor.columns == ["n"]
    assert cursor.rowcount == 0


def test_write_only_query_columns(db):
    """Write-only queries should have no columns."""
    cursor = db.execute("CREATE (:Test)")
    assert cursor.columns == []
    assert cursor.rowcount == -1


# ============================================================
# Error handling
# ============================================================


def test_error_handling(db):
    # A syntax error is a ProgrammingError per the DB-API exception mapping.
    with pytest.raises(relate.ProgrammingError):
        db.execute("INVALID SYNTAX")


def test_error_recovery(db):
    """Engine should continue working after an error."""
    with pytest.raises(relate.ProgrammingError):
        db.execute("INVALID SYNTAX")

    cursor = db.execute("RETURN 1 AS ok")
    assert cursor.fetchone()["ok"] == 1


def test_multiple_errors_recovery(db):
    """Multiple consecutive errors should not corrupt state."""
    for _ in range(3):
        with pytest.raises(relate.ProgrammingError):
            db.execute("NOPE")

    cursor = db.execute("RETURN 'ok' AS status")
    assert cursor.fetchone()["status"] == "ok"


# ============================================================
# executemany
# ============================================================


def test_executemany(db):
    db.executemany(
        "CREATE (:Person {name: $name})",
        [{"name": "Alice"}, {"name": "Bob"}, {"name": "Charlie"}],
    )
    cursor = db.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 3


def test_executemany_empty_list(db):
    """executemany with empty list should return empty cursor."""
    cursor = db.executemany("CREATE (:Test {v: $v})", [])
    assert cursor.columns == []
    assert cursor.fetchall() == []


def test_executemany_single_item(db):
    cursor = db.executemany("CREATE (:Test {v: $v})", [{"v": 42}])
    assert cursor.stats["nodes_created"] == 1


def test_executemany_returns_last_cursor(db):
    """executemany should return cursor from the last execution."""
    cursor = db.executemany(
        "CREATE (:Test {v: $v})",
        [{"v": 1}, {"v": 2}, {"v": 3}],
    )
    # Stats reflect only last execution
    assert cursor.stats["nodes_created"] == 1


# ============================================================
# Semicolon and whitespace tolerance
# ============================================================


def test_query_without_semicolon(db):
    """Queries without trailing semicolons should work."""
    cursor = db.execute("RETURN 42 AS answer")
    assert cursor.fetchone()["answer"] == 42


def test_query_with_semicolon(db):
    """Queries with trailing semicolons should also work."""
    cursor = db.execute("RETURN 42 AS answer;")
    assert cursor.fetchone()["answer"] == 42


def test_query_with_trailing_whitespace(db):
    cursor = db.execute("  RETURN 42 AS answer  ")
    assert cursor.fetchone()["answer"] == 42


# ============================================================
# Cursor integration
# ============================================================


def test_cursor_iteration(db):
    db.execute("CREATE (:N {v: 1})")
    db.execute("CREATE (:N {v: 2})")
    cursor = db.execute("MATCH (n:N) RETURN n.v ORDER BY n.v")
    values = [row["n.v"] for row in cursor]
    assert values == [1, 2]


def test_cursor_columns_match_return_aliases(db):
    cursor = db.execute("RETURN 1 AS foo, 2 AS bar")
    assert cursor.columns == ["foo", "bar"]


def test_cursor_description_integration(db):
    import relate

    cursor = db.execute("RETURN 1 AS x, 2 AS y")
    desc = cursor.description
    assert [d[0] for d in desc] == ["x", "y"]
    # type_code is inferred from the first row's value: integers -> NUMBER.
    assert desc[0][1] == relate.NUMBER
    assert desc[1][1] == relate.NUMBER
    # The remaining five description fields are unpopulated.
    assert desc[0][2:] == (None, None, None, None, None)


# ============================================================
# Persistence
# ============================================================


def test_persistent_database(tmp_path):
    db_path = str(tmp_path / "database")

    db = relate.connect(db_path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.close()

    db2 = relate.connect(db_path)
    cursor = db2.execute("MATCH (p:Person) RETURN p.name")
    assert cursor.fetchone()["p.name"] == "Alice"
    db2.close()


def test_persistent_database_multiple_writes(tmp_path):
    """Multiple write sessions should accumulate data."""
    db_path = str(tmp_path / "database")

    db = relate.connect(db_path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.close()

    db = relate.connect(db_path)
    db.execute("CREATE (:Person {name: 'Bob'})")
    db.close()

    db = relate.connect(db_path)
    cursor = db.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 2
    db.close()


def test_persistent_database_flush(tmp_path):
    """flush() checkpoints data to the Relate directory."""
    db_path = str(tmp_path / "database")

    db = relate.connect(db_path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.flush()
    db.close()

    db2 = relate.connect(db_path)
    cursor = db2.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 1
    db2.close()


# ============================================================
# Connection state
# ============================================================


def test_closed_connection():
    db = relate.connect(":memory:")
    db.close()
    assert db.closed
    with pytest.raises(relate.ProgrammingError):
        db.execute("RETURN 1")


def test_closed_property_before_close():
    db = relate.connect(":memory:")
    assert not db.closed
    db.close()


def test_executemany_on_closed_connection():
    db = relate.connect(":memory:")
    db.close()
    with pytest.raises(relate.ProgrammingError):
        db.executemany("CREATE (:T {v: $v})", [{"v": 1}])


def test_flush_on_closed_connection():
    db = relate.connect(":memory:")
    db.close()
    with pytest.raises(relate.ProgrammingError):
        db.flush()


# ============================================================
# Multiple connections
# ============================================================


def test_multiple_in_memory_connections():
    """Two in-memory connections should be independent."""
    db1 = relate.connect(":memory:")
    db2 = relate.connect(":memory:")

    db1.execute("CREATE (:Person {name: 'Alice'})")
    db2.execute("CREATE (:Animal {name: 'Cat'})")

    c1 = db1.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert c1.fetchone()["c"] == 1

    c2 = db2.execute("MATCH (p:Person) RETURN count(*) AS c")
    assert c2.fetchone()["c"] == 0

    db1.close()
    db2.close()


# ============================================================
# Aggregation queries
# ============================================================


def test_aggregation_count(db):
    db.executemany("CREATE (:N {v: $v})", [{"v": i} for i in range(5)])
    cursor = db.execute("MATCH (n:N) RETURN count(*) AS c")
    assert cursor.fetchone()["c"] == 5


def test_aggregation_sum(db):
    db.executemany("CREATE (:N {v: $v})", [{"v": i} for i in range(1, 4)])
    cursor = db.execute("MATCH (n:N) RETURN sum(n.v) AS s")
    assert cursor.fetchone()["s"] == 6


def test_aggregation_collect(db):
    db.executemany("CREATE (:N {v: $v})", [{"v": i} for i in range(1, 4)])
    cursor = db.execute("MATCH (n:N) RETURN collect(n.v) AS vals")
    row = cursor.fetchone()
    assert sorted(row["vals"]) == [1, 2, 3]


# ============================================================
# Unicode and special characters
# ============================================================


def test_unicode_property(db):
    db.execute("CREATE (:Person {name: 'René'})")
    cursor = db.execute("MATCH (p:Person) RETURN p.name")
    assert cursor.fetchone()["p.name"] == "René"


def test_unicode_emoji(db):
    db.execute("CREATE (:Note {text: 'Hello \U0001f44b'})")
    cursor = db.execute("MATCH (n:Note) RETURN n.text")
    assert cursor.fetchone()["n.text"] == "Hello \U0001f44b"


def test_string_with_quotes(db):
    db.execute('CREATE (:Note {text: "has \'quotes\'"})')
    cursor = db.execute("MATCH (n:Note) RETURN n.text")
    row = cursor.fetchone()
    assert "quotes" in row["n.text"]
