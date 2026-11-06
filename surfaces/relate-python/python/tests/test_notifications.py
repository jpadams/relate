"""Query notifications (ADR-0363) on the native result and the DB-API summary.

The engine derives notifications only where it builds a plan tree
(``EXPLAIN``/``PROFILE``); the plain execution path returns an empty list.
Both the native ``QueryResult`` and ``Cursor.consume()`` expose the same
engine-native entries — ``code``, ``gql_status``, ``severity``,
``category``, ``title``, ``description``, ``remedy`` and, on
operator-scoped kinds, ``operator_id`` — while cursor iteration stays
rows-only.
"""

import relate

CARTESIAN_STATEMENT = "MATCH (a:Person), (b:City) RETURN a, b"
CARTESIAN_CODE = "Neo.ClientNotification.Statement.CartesianProduct"

NATIVE_KEYS = {
    "code",
    "gql_status",
    "severity",
    "category",
    "title",
    "description",
    "remedy",
    "operator_id",
}


def _seed_person_and_city(execute):
    execute("CREATE (:Person)")
    execute("CREATE (:City)")


def _assert_cartesian_notification(notifications):
    """The disconnected two-label MATCH raises exactly the cartesian
    notification, in the engine-native shape."""
    assert [n["code"] for n in notifications] == [CARTESIAN_CODE]
    entry = notifications[0]
    assert set(entry) == NATIVE_KEYS
    assert entry["gql_status"] == "03N90"
    assert entry["severity"] == "INFORMATION"
    assert entry["category"] == "PERFORMANCE"
    assert entry["title"] == "Cartesian product"
    assert entry["description"] == "the MATCH patterns share no join anchor"
    assert "join anchor" in entry["remedy"]
    assert isinstance(entry["operator_id"], int)


def test_native_result_exposes_notifications():
    db = relate.Database(":memory:")
    try:
        _seed_person_and_city(db.execute)

        explained = db.execute(f"EXPLAIN {CARTESIAN_STATEMENT}")
        _assert_cartesian_notification(explained.notifications)

        plain = db.execute(CARTESIAN_STATEMENT)
        assert plain.notifications == []
    finally:
        db.close()


def test_cursor_consume_carries_notifications(db):
    _seed_person_and_city(db.execute)

    summary = db.execute(f"EXPLAIN {CARTESIAN_STATEMENT}").consume()
    _assert_cartesian_notification(summary.notifications)
    # Idempotent, like the rest of the summary.
    repeated = db.execute(f"EXPLAIN {CARTESIAN_STATEMENT}").consume()
    assert repeated.notifications == summary.notifications

    plain = db.execute(CARTESIAN_STATEMENT).consume()
    assert plain.notifications == []


def test_cursor_iteration_stays_rows_only(db):
    _seed_person_and_city(db.execute)

    # EXPLAIN's plan rides QueryResult.plan (ADR-0359), not the cursor:
    # iteration yields no rows, while the summary carries the plan-time
    # notification list.
    explained = db.execute(f"EXPLAIN {CARTESIAN_STATEMENT}")
    assert list(explained) == [], "EXPLAIN has no rows to iterate"
    _assert_cartesian_notification(explained.consume().notifications)

    # PROFILE keeps the wrapped statement's own rows (ADR-0359), so
    # iteration yields Records only — never the notification entries.
    cursor = db.execute(f"PROFILE {CARTESIAN_STATEMENT}")
    rows = list(cursor)
    assert rows, "PROFILE returns the wrapped statement's data rows"
    assert all(isinstance(row, relate.Record) for row in rows)
    assert all("code" not in row for row in rows)
    _assert_cartesian_notification(cursor.consume().notifications)
