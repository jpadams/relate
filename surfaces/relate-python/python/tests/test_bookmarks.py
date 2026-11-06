"""Bookmarks: the engine's commit sequence, monotonic per store
(ADR-0502 §7) — on auto-commit results and on explicit commit alike."""


def _seq(bookmark):
    prefix, n = bookmark.split(":")
    assert prefix == "relate"
    return int(n)


def test_two_autocommit_writes_return_increasing_bookmarks(db):
    first = db.execute("CREATE (:B)").consume().bookmark
    second = db.execute("CREATE (:B)").consume().bookmark
    assert first is not None and second is not None
    assert _seq(second) > _seq(first)


def test_native_query_result_carries_the_bookmark(native_db):
    first = native_db.execute("CREATE (:B)").bookmark
    second = native_db.execute("CREATE (:B)").bookmark
    assert _seq(second) > _seq(first)
    assert native_db.execute("MATCH (n) RETURN count(n) AS c").bookmark is None


def test_read_has_no_bookmark(db):
    db.execute("CREATE (:B)")
    assert db.execute("MATCH (n:B) RETURN n").consume().bookmark is None


def test_explicit_commit_returns_increasing_bookmarks(db):
    before = db.execute("CREATE (:B)").consume().bookmark
    db.autocommit = False
    cur = db.execute("CREATE (:B)")
    # Inside the transaction nothing is committed yet.
    assert cur.consume().bookmark is None
    first = db.commit()
    db.execute("CREATE (:B)")
    second = db.commit()
    assert _seq(before) < _seq(first) < _seq(second)


def test_commit_without_transaction_returns_none(db):
    assert db.commit() is None
    db.autocommit = False
    assert db.commit() is None


def test_native_commit_transaction_returns_the_bookmark(native_db):
    native_db.begin_transaction()
    native_db.execute("CREATE (:B)")
    assert _seq(native_db.commit_transaction()) > 0
