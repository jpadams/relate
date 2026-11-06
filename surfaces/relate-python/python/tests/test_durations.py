"""Durations: lossless months/days/seconds/nanoseconds (ADR-0502 §1) —
``relate.Duration`` at every depth, ``timedelta`` parameters still
bind, ``to_timedelta()`` the checked projection."""

import datetime

import pytest

import relate


def test_pt36h_is_not_p1dt12h(db):
    row = db.execute(
        "RETURN duration('PT36H') AS a, duration('P1DT12H') AS b"
    ).fetchone()
    a, b = row["a"], row["b"]
    assert isinstance(a, relate.Duration) and isinstance(b, relate.Duration)
    assert (a.months, a.days, a.seconds, a.nanoseconds) == (0, 0, 129600, 0)
    assert (b.months, b.days, b.seconds, b.nanoseconds) == (0, 1, 43200, 0)
    assert a != b
    # The timedelta projection is exactly what merges them.
    assert a.to_timedelta() == b.to_timedelta()


def test_month_and_sub_microsecond_duration_components(db):
    d = db.execute("RETURN duration('P1Y2M3DT4.000000005S') AS d").fetchone()["d"]
    assert (d.months, d.days, d.seconds, d.nanoseconds) == (14, 3, 4, 5)
    assert str(d) == "P1Y2M3DT4.000000005S"
    with pytest.raises(ValueError):
        d.to_timedelta()


def test_duration_param_roundtrips_losslessly(db):
    d = relate.Duration(months=14, days=3, seconds=4, nanoseconds=5)
    back = db.execute("RETURN $d AS d", {"d": d}).fetchone()["d"]
    assert back == d
    assert db.execute(
        "RETURN $d = duration('P1Y2M3DT4.000000005S') AS same", {"d": d}
    ).fetchone()["same"]


def test_duration_equality_and_hash_follow_the_engine():
    a = relate.Duration(seconds=1, nanoseconds=500_000_000)
    b = relate.Duration(nanoseconds=1_500_000_000)
    assert a == b and hash(a) == hash(b)
    assert relate.Duration(days=1) != relate.Duration(seconds=86400)
    assert repr(relate.Duration(months=1)) == (
        "Duration(months=1, days=0, seconds=0, nanoseconds=0)"
    )


def test_duration_is_typed_at_every_depth(db):
    db.execute(
        "CREATE (:D {d: duration('PT36H')})-[:R {d: duration('P1M')}]->(:D)"
    )
    row = db.execute(
        "MATCH (n:D)-[r:R]->() RETURN n, r, [duration('PT1S')] AS l, "
        "{inner: {d: duration('P2D')}} AS m"
    ).fetchone()
    assert row["n"]["d"] == relate.Duration(seconds=129600)
    assert row["r"]["d"] == relate.Duration(months=1)
    assert row["l"] == [relate.Duration(seconds=1)]
    assert row["m"]["inner"]["d"] == relate.Duration(days=2)


def test_timedelta_param_still_binds(db):
    # timedelta itself normalizes 36 hours into one day and 12 hours.
    td = datetime.timedelta(hours=36)
    back = db.execute("RETURN $td AS d", {"td": td}).fetchone()["d"]
    assert back == relate.Duration(days=1, seconds=43200)
