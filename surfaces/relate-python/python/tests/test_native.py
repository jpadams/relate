"""Tests for the native Database class (relate._native) re-exported as relate.Database."""

import pytest

import relate
from relate import Database, QueryResult


# ============================================================
# Construction
# ============================================================


def test_default_is_in_memory():
    db = Database()
    assert db.node_count() == 0
    assert db.relationship_count() == 0


def test_explicit_memory_path():
    db = Database(":memory:")
    assert db.node_count() == 0


def test_file_path(tmp_path):
    path = tmp_path / "graph"
    db = Database(str(path))
    assert db.node_count() == 0
    db.close()
    assert path.is_dir()
    assert (path / "wal").is_dir()


def test_path_suffix_does_not_select_a_storage_backend(tmp_path):
    """Persistent paths are Relate directories regardless of suffix."""
    path = tmp_path / "graph.lance"
    db = Database(str(path))
    db.close()
    assert path.is_dir()


def test_legacy_json_file_is_not_auto_imported(tmp_path):
    path = tmp_path / "graph.json"
    path.write_text('{"nodes":[],"relationships":[]}')
    with pytest.raises(RuntimeError):
        Database(str(path))


# ============================================================
# execute
# ============================================================


def test_execute_returns_query_result():
    db = Database()
    result = db.execute("RETURN 1 AS x")
    assert isinstance(result, QueryResult)
    assert result.columns == ["x"]
    assert result.data == [{"x": 1}]


def test_execute_with_params():
    db = Database()
    db.execute("CREATE (:Person {name: $name})", {"name": "Alice"})
    result = db.execute("MATCH (p:Person) RETURN p.name")
    assert result.data == [{"p.name": "Alice"}]


def test_execute_stats():
    db = Database()
    result = db.execute("CREATE (:Person {name: 'Alice'})")
    assert result.stats["nodes_created"] == 1
    assert result.stats["properties_set"] == 1


def test_execute_phase_timing_split():
    # ADR-0491: the native QueryResult carries the engine-stamped PARSE /
    # PLAN / EXECUTE split of elapsed_ms — disjoint sub-windows whose sum
    # never exceeds the total.
    db = Database()
    result = db.execute("RETURN 1 AS x")
    assert result.elapsed_ms >= 0.0
    assert result.parse_ms >= 0.0
    assert result.plan_ms >= 0.0
    assert result.execute_ms >= 0.0
    assert result.parse_ms + result.plan_ms + result.execute_ms <= result.elapsed_ms


def test_execute_error_raises_runtime_error():
    db = Database()
    with pytest.raises(RuntimeError):
        db.execute("INVALID SYNTAX")


def test_query_result_repr():
    db = Database()
    result = db.execute("RETURN 1 AS x")
    assert "QueryResult" in repr(result)


# ============================================================
# execute_arrow
# ============================================================


def test_execute_arrow_returns_bytes():
    db = Database()
    db.execute("CREATE (:Person {name: 'Alice', age: 30})")
    buf = db.execute_arrow("MATCH (p:Person) RETURN p.name, p.age")
    assert isinstance(buf, bytes)
    assert len(buf) > 0


def test_execute_arrow_pyarrow_roundtrip():
    pa = pytest.importorskip("pyarrow")
    db = Database()
    db.execute("CREATE (:Person {name: 'Alice', age: 30})")
    buf = db.execute_arrow("MATCH (p:Person) RETURN p.name AS name, p.age AS age")
    reader = pa.ipc.open_stream(buf)
    table = reader.read_all()
    assert table.num_rows == 1
    assert table.column("name").to_pylist() == ["Alice"]
    assert table.column("age").to_pylist() == [30]


# ============================================================
# Introspection
# ============================================================


def test_node_and_relationship_counts():
    db = Database()
    db.execute("CREATE (:A)-[:REL]->(:B)")
    assert db.node_count() == 2
    assert db.relationship_count() == 1


def test_labels_and_relationship_types():
    db = Database()
    db.execute("CREATE (:Person)-[:KNOWS]->(:Animal)")
    assert sorted(db.labels()) == ["Animal", "Person"]
    assert db.relationship_types() == ["KNOWS"]


def test_repr():
    db = Database()
    assert repr(db) == "Database(nodes=0, relationships=0)"


# ============================================================
# reset
# ============================================================


def test_reset_in_memory():
    db = Database()
    db.execute("CREATE (:Person)")
    assert db.node_count() == 1
    db.reset()
    assert db.node_count() == 0


def test_reset_file_backed_reloads_from_disk(tmp_path):
    """For file-backed databases, reset() flushes and reopens the same path."""
    path = str(tmp_path / "graph")
    db = Database(path)
    db.execute("CREATE (:Person)")
    db.reset()
    assert db.node_count() == 1  # data was flushed and reloaded
    db.close()


# ============================================================
# flush / close persistence
# ============================================================


def test_flush_persists_to_disk(tmp_path):
    path = str(tmp_path / "graph")
    db = Database(path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.flush()
    db.close()

    db2 = Database(path)
    assert db2.node_count() == 1
    db2.close()


def test_close_persists_to_disk(tmp_path):
    path = str(tmp_path / "graph")
    db = Database(path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    db.close()

    db2 = Database(path)
    assert db2.node_count() == 1


def test_garbage_collection_flushes(tmp_path):
    """Dropping the Database object flushes file-backed data (best-effort)."""
    path = str(tmp_path / "graph")
    db = Database(path)
    db.execute("CREATE (:Person {name: 'Alice'})")
    del db

    db2 = Database(path)
    assert db2.node_count() == 1


def test_flush_in_memory_is_noop():
    db = Database()
    db.execute("CREATE (:Person)")
    db.flush()  # should not raise
    assert db.node_count() == 1


def test_read_only_rejects_writes():
    db = Database(":memory:", read_only=True)
    db.execute("RETURN 1 AS n")
    with pytest.raises(Exception, match="read-only"):
        db.execute("CREATE ()")
    db.flush()
    db.close()

    writable = Database(":memory:")
    writable.execute("CREATE ()")
    assert writable.node_count() == 1
