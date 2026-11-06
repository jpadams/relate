"""Connection.commit()/rollback() wired to engine transactions (decision 3),
and the autocommit default."""

import pytest

import relate
from relate._native import RelateNativeError


def _count(db):
    return db.execute("MATCH (n) RETURN count(*) AS c").fetchone()["c"]


class _FakeDb:
    """Duck-typed native ``Database`` stand-in: ``transaction_active``
    raises ``probe_error`` when one is given (else reports an open
    transaction), ``rollback_transaction`` raises ``error`` when one is
    given, and ``close`` only records that it ran."""

    def __init__(self, error=None, probe_error=None):
        self.closed = False
        self._error = error
        self._probe_error = probe_error

    def transaction_active(self):
        if self._probe_error is not None:
            raise self._probe_error
        return True

    def rollback_transaction(self):
        if self._error is not None:
            raise self._error

    def close(self):
        self.closed = True


def _conn_with(fake):
    """A live connection whose native handle is swapped for `fake`."""
    conn = relate.connect()
    conn._db = fake
    return conn


# ============================================================
# Autocommit default
# ============================================================


def test_autocommit_default_true(db):
    assert db.autocommit is True


def test_autocommit_writes_are_immediately_visible(db):
    db.execute("CREATE (:N)")
    assert _count(db) == 1


def test_commit_rollback_noop_in_autocommit(db):
    db.execute("CREATE (:N)")
    db.commit()  # no-op, no error
    db.rollback()  # no-op, no error
    assert _count(db) == 1


def test_supports_transactions(db):
    assert db._db.supports_transactions() is True


def test_transaction_state_is_the_engines_own_fact(db):
    """The binding keeps no mirror of the engine's transaction state
    (LABS-944): every decision point — the begin guard, commit, rollback,
    the autocommit setter — asks the engine through the one
    ``_transaction_open`` accessor, and a Connection carries no
    ``_in_transaction`` copy to diverge from it."""
    assert not hasattr(db, "_in_transaction")
    db.autocommit = False
    db.execute("CREATE (:Mirrorless)")
    assert db._transaction_open()
    db.commit()
    assert not db._transaction_open()


def test_transaction_probe_maps_native_errors():
    """The engine's ``transaction_active()`` read crosses the same
    error-mapping boundary as every other native call (LABS-944): a
    native failure on the probe surfaces as the mapped DB-API error —
    and ``close()`` still runs the native close after one (the mapped
    error is captured like any rollback failure, never skipped past)."""
    fake = _FakeDb(probe_error=RelateNativeError("probe boom"))
    conn = _conn_with(fake)
    with pytest.raises(relate.DatabaseError):
        conn.commit()
    with pytest.raises(relate.DatabaseError):
        conn.close()
    assert fake.closed, "native close was skipped after a failed probe"
    assert conn.closed


# ============================================================
# Explicit transactions (autocommit = False)
# ============================================================


def test_rollback_discards_changes(db):
    db.autocommit = False
    db.execute("CREATE (:Person {name: 'Temp'})")
    assert _count(db) == 1  # visible within the transaction
    db.rollback()
    assert _count(db) == 0  # discarded


def test_commit_persists_changes(db):
    db.autocommit = False
    db.execute("CREATE (:Person {name: 'Keep'})")
    db.commit()
    assert _count(db) == 1


def test_rollback_via_cursor(db):
    db.autocommit = False
    cur = db.cursor()
    cur.execute("CREATE (:X)")
    db.rollback()
    cur.execute("MATCH (n:X) RETURN count(*) AS c")
    assert cur.fetchone()["c"] == 0


def test_commit_via_cursor(db):
    db.autocommit = False
    cur = db.cursor()
    cur.execute("CREATE (:X)")
    db.commit()
    cur.execute("MATCH (n:X) RETURN count(*) AS c")
    assert cur.fetchone()["c"] == 1


def test_switching_to_autocommit_commits_open_transaction(db):
    db.autocommit = False
    db.execute("CREATE (:Keep)")
    db.autocommit = True  # should commit the open transaction
    assert _count(db) == 1


def test_multiple_statements_in_one_transaction(db):
    db.autocommit = False
    db.execute("CREATE (:A)")
    db.execute("CREATE (:B)")
    db.execute("CREATE (:C)")
    assert _count(db) == 3
    db.rollback()
    assert _count(db) == 0


def test_transaction_persists_to_disk_on_commit(tmp_path):
    path = str(tmp_path / "txn")
    conn = relate.connect(path)
    conn.autocommit = False
    conn.execute("CREATE (:Person {name: 'Alice'})")
    conn.commit()
    conn.close()

    conn2 = relate.connect(path)
    assert _count(conn2) == 1
    conn2.close()


def test_failed_commit_keeps_transaction_open_for_rollback(db):
    """A commit the engine refuses (the transaction failed) leaves the
    engine's transaction open, so rollback() still discards the
    transaction's writes."""
    db.autocommit = False
    db.execute("CREATE (:Person {name: 'Doomed'})")
    with pytest.raises(relate.DatabaseError):
        db.execute("RETURN nosuchvar")
    with pytest.raises(relate.DatabaseError, match="rollback required"):
        db.commit()
    assert db._db.transaction_active()
    db.rollback()
    assert not db._db.transaction_active()
    db.autocommit = True
    assert _count(db) == 0


def test_close_rolls_back_open_transaction(tmp_path):
    path = str(tmp_path / "txn2")
    conn = relate.connect(path)
    conn.autocommit = False
    conn.execute("CREATE (:Person {name: 'Uncommitted'})")
    conn.close()  # implicit rollback per DB-API

    conn2 = relate.connect(path)
    assert _count(conn2) == 0
    conn2.close()


def test_close_runs_native_close_when_rollback_fails():
    """Regression: a failed rollback must not skip the native close (the
    ``error or _capture_native_error(...)`` spelling short-circuited it)."""
    fake = _FakeDb(RelateNativeError("rollback boom"))
    conn = _conn_with(fake)
    with pytest.raises(relate.DatabaseError):
        conn.close()
    assert fake.closed, "native close was skipped after a failed rollback"
    assert conn.closed


def test_close_runs_on_runtime_error_rollback():
    """pyo3 also surfaces native failures as plain RuntimeError; the close
    guarantee covers that shape too (mapped to OperationalError)."""
    fake = _FakeDb(RuntimeError("native boom"))
    conn = _conn_with(fake)
    with pytest.raises(relate.OperationalError):
        conn.close()
    assert fake.closed
    assert conn.closed
