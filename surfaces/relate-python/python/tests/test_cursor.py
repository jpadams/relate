"""Unit tests for the Cursor class (pure Python, no native module needed)."""

from relate.cursor import Cursor


# --- Basic fetch operations ---


def test_fetchone():
    cursor = Cursor(
        columns=["name"], rows=[{"name": "Alice"}, {"name": "Bob"}], stats={}
    )
    assert cursor.fetchone() == {"name": "Alice"}
    assert cursor.fetchone() == {"name": "Bob"}
    assert cursor.fetchone() is None


def test_fetchone_repeated_none():
    """Calling fetchone past exhaustion should keep returning None."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}], stats={})
    assert cursor.fetchone() == {"x": 1}
    assert cursor.fetchone() is None
    assert cursor.fetchone() is None
    assert cursor.fetchone() is None


def test_fetchall():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    assert cursor.fetchall() == [{"x": 1}, {"x": 2}]
    assert cursor.fetchall() == []


def test_fetchmany():
    cursor = Cursor(
        columns=["x"], rows=[{"x": 1}, {"x": 2}, {"x": 3}], stats={}
    )
    assert cursor.fetchmany(2) == [{"x": 1}, {"x": 2}]
    assert cursor.fetchmany(2) == [{"x": 3}]
    assert cursor.fetchmany(2) == []


def test_fetchmany_default_size():
    """Default size is 1."""
    cursor = Cursor(
        columns=["x"], rows=[{"x": 1}, {"x": 2}, {"x": 3}], stats={}
    )
    assert cursor.fetchmany() == [{"x": 1}]
    assert cursor.fetchmany() == [{"x": 2}]


def test_fetchmany_size_zero():
    """fetchmany(0) should return empty list."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}], stats={})
    assert cursor.fetchmany(0) == []


def test_fetchmany_size_larger_than_remaining():
    """fetchmany with size larger than remaining rows returns what's left."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    assert cursor.fetchmany(100) == [{"x": 1}, {"x": 2}]
    assert cursor.fetchmany(100) == []


# --- Mixed fetch operations ---


def test_fetchone_then_fetchall():
    """fetchone advances position, then fetchall gets the rest."""
    cursor = Cursor(
        columns=["x"], rows=[{"x": 1}, {"x": 2}, {"x": 3}], stats={}
    )
    assert cursor.fetchone() == {"x": 1}
    assert cursor.fetchall() == [{"x": 2}, {"x": 3}]


def test_fetchone_then_fetchmany():
    cursor = Cursor(
        columns=["x"], rows=[{"x": 1}, {"x": 2}, {"x": 3}], stats={}
    )
    assert cursor.fetchone() == {"x": 1}
    assert cursor.fetchmany(1) == [{"x": 2}]
    assert cursor.fetchone() == {"x": 3}
    assert cursor.fetchone() is None


def test_fetchall_then_fetchone():
    """After fetchall, fetchone returns None."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}], stats={})
    cursor.fetchall()
    assert cursor.fetchone() is None


# --- Empty cursor ---


def test_empty_cursor_fetchone():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert cursor.fetchone() is None


def test_empty_cursor_fetchall():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert cursor.fetchall() == []


def test_empty_cursor_fetchmany():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert cursor.fetchmany(5) == []


def test_empty_cursor_len():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert len(cursor) == 0


def test_empty_cursor_iteration():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert list(cursor) == []


# --- Iteration ---


def test_iteration():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    assert list(cursor) == [{"x": 1}, {"x": 2}]


def test_iteration_respects_fetch_position():
    """Per DB-API, iterating a cursor advances (and shares) the fetch position:
    rows already fetched are not yielded again."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    cursor.fetchone()  # consume the first row
    assert list(cursor) == [{"x": 2}]  # only the remaining row


def test_second_iteration_is_empty():
    """Per DB-API, a cursor is consumed by iteration; a second pass is empty."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    assert list(cursor) == [{"x": 1}, {"x": 2}]
    assert list(cursor) == []


# --- len ---


def test_len():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    assert len(cursor) == 2


def test_len_unchanged_after_fetch():
    """len() always returns total row count, not remaining."""
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}], stats={})
    cursor.fetchone()
    assert len(cursor) == 2


# --- Columns ---


def test_columns():
    cursor = Cursor(columns=["a", "b", "c"], rows=[], stats={})
    assert cursor.columns == ["a", "b", "c"]


def test_columns_empty():
    cursor = Cursor(columns=None, rows=None, stats=None)
    assert cursor.columns == []


def test_columns_returns_copy():
    """Modifying returned columns should not affect the cursor."""
    cursor = Cursor(columns=["a", "b"], rows=[], stats={})
    cols = cursor.columns
    cols.append("c")
    assert cursor.columns == ["a", "b"]


def test_columns_special_characters():
    """Column names with dots and spaces (common in Cypher results)."""
    cursor = Cursor(columns=["p.name", "p.age", "count(*)"], rows=[], stats={})
    assert cursor.columns == ["p.name", "p.age", "count(*)"]


# --- Stats ---


def test_stats():
    cursor = Cursor(
        columns=None,
        rows=None,
        stats={"nodes_created": 3, "properties_set": 5},
    )
    assert cursor.stats == {"nodes_created": 3, "properties_set": 5}


def test_stats_empty():
    cursor = Cursor(columns=None, rows=None, stats=None)
    assert cursor.stats == {}


def test_stats_returns_copy():
    """Modifying returned stats should not affect the cursor."""
    cursor = Cursor(columns=None, rows=None, stats={"nodes_created": 1})
    s = cursor.stats
    s["nodes_created"] = 999
    assert cursor.stats == {"nodes_created": 1}


# --- Description (DB-API 2.0) ---


def test_description():
    cursor = Cursor(columns=["a", "b"], rows=[], stats={})
    assert cursor.description == [
        ("a", None, None, None, None, None, None),
        ("b", None, None, None, None, None, None),
    ]


def test_description_none():
    cursor = Cursor(columns=None, rows=None, stats=None)
    assert cursor.description is None


def test_description_single_column():
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert len(cursor.description) == 1
    assert cursor.description[0][0] == "x"


# --- rowcount ---


def test_rowcount_with_columns():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}], stats={})
    assert cursor.rowcount == 1


def test_rowcount_multiple_rows():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}, {"x": 2}, {"x": 3}], stats={})
    assert cursor.rowcount == 3


def test_rowcount_empty_result_with_columns():
    """Columns present but no rows — rowcount should be 0."""
    cursor = Cursor(columns=["x"], rows=[], stats={})
    assert cursor.rowcount == 0


def test_rowcount_no_columns():
    cursor = Cursor(columns=None, rows=None, stats={"nodes_created": 1})
    assert cursor.rowcount == -1


# --- close ---


def test_close_is_noop():
    cursor = Cursor(columns=["x"], rows=[{"x": 1}], stats={})
    cursor.close()  # Should not raise
    assert cursor.fetchone() == {"x": 1}  # Still works after close


# --- Row data immutability ---


def test_row_data_types():
    """Cursor handles various data types in rows."""
    rows = [{"s": "hello", "i": 42, "f": 3.14, "b": True, "n": None}]
    cursor = Cursor(columns=["s", "i", "f", "b", "n"], rows=rows, stats={})
    row = cursor.fetchone()
    assert row["s"] == "hello"
    assert row["i"] == 42
    assert row["f"] == 3.14
    assert row["b"] is True
    assert row["n"] is None
