"""Tests for context manager and cleanup behavior."""

import pytest

import relate


def test_context_manager():
    with relate.connect(":memory:") as db:
        db.execute("CREATE (:Test)")
        cursor = db.execute("MATCH (t:Test) RETURN count(*) AS c")
        assert cursor.fetchone()["c"] == 1
    assert db.closed


def test_context_manager_on_exception():
    """Connection should be closed even if an exception occurs."""
    try:
        with relate.connect(":memory:") as db:
            db.execute("CREATE (:Test)")
            raise ValueError("test error")
    except ValueError:
        pass
    assert db.closed


def test_double_close():
    """Calling close() twice should not raise."""
    db = relate.connect(":memory:")
    db.close()
    db.close()  # Should be a no-op
    assert db.closed


def test_context_manager_query_result_accessible_after():
    """Results obtained inside the context should remain accessible after."""
    with relate.connect(":memory:") as db:
        cursor = db.execute("RETURN 42 AS answer")
        rows = cursor.fetchall()
    assert rows == [{"answer": 42}]


def test_context_manager_on_query_error():
    """Connection should stay usable after a query error, and still close."""
    with relate.connect(":memory:") as db:
        try:
            # A syntax error maps to ProgrammingError (a DB-API DatabaseError).
            db.execute("INVALID SYNTAX")
        except relate.ProgrammingError:
            pass
        # Connection should still work after catching the error
        cursor = db.execute("RETURN 1 AS ok")
        assert cursor.fetchone()["ok"] == 1
    assert db.closed


def test_nested_context_managers():
    """Multiple context managers should work independently."""
    with relate.connect(":memory:") as db1:
        db1.execute("CREATE (:A)")
        with relate.connect(":memory:") as db2:
            db2.execute("CREATE (:B)")
            c2 = db2.execute("MATCH (n:A) RETURN count(*) AS c")
            assert c2.fetchone()["c"] == 0  # separate database
        assert db2.closed
        # db1 still works
        c1 = db1.execute("MATCH (n:A) RETURN count(*) AS c")
        assert c1.fetchone()["c"] == 1
    assert db1.closed
