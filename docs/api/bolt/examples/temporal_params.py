#!/usr/bin/env python3
"""Send temporal, duration and point parameters over Bolt with the official
Neo4j driver, and prove each decodes to the value its Cypher constructor
produces (ADR-0177); read temporal and point results back as the driver's
native types (ADR-0325, ADR-0326); and round-trip a bytes parameter
(ADR-0328).

Start the server first — behind the `--bolt-identity` compat shim,
because the current python driver validates the HELLO `server` agent and
rejects anything not starting `Neo4j/` (ADR-0293):
  cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory: --bolt-identity Neo4j/5.26.0
The port comes from RELATE_BOLT_PORT_IDENTITY (scripts/bolt-smoke.sh
exports it beside the spawn; unset is a configuration error).
"""
from _topology import bolt_uri
from datetime import date, datetime, time, timedelta, timezone

import pytz
from neo4j import GraphDatabase
from neo4j.spatial import CartesianPoint, WGS84Point
from neo4j.time import Date, DateTime, Duration, Time


def assert_parameter_equals(driver, value, constructor, label):
    """Assert `$p = <constructor>` answers true for this Python value."""
    records, _, _ = driver.execute_query(
        f"RETURN $p = {constructor} AS ok", p=value
    )
    assert records[0]["ok"] is True, f"{label}: $p != {constructor}"


def main():
    driver = GraphDatabase.driver(
        bolt_uri("RELATE_BOLT_PORT_IDENTITY"), auth=("neo4j", "unused"))

    # Temporal parameters 0x44/0x54/0x74: date, fixed-offset time, localtime
    # (LABS-323 findings D-26/D-27/D-28). A `datetime.time` needs a
    # fixed-offset tzinfo whose `utcoffset` accepts a time; the stdlib
    # `timezone` raises TypeError there, so use pytz (a neo4j dependency).
    offset = pytz.FixedOffset(60)
    assert_parameter_equals(
        driver, date(1970, 1, 1), "date('1970-01-01')", "date (D-26)"
    )
    assert_parameter_equals(
        driver, time(12, 30, tzinfo=offset), "time('12:30:00+01:00')", "time (D-27)"
    )
    assert_parameter_equals(
        driver, time(12, 30), "localtime('12:30:00')", "localtime (D-28)"
    )
    print("temporal parameters ok: date, time, localtime")

    # Temporal parameters 0x46/0x66/0x64 and duration 0x45 (D-29..D-32). A
    # Python datetime only carries microseconds, so the 42-nanosecond
    # worked examples match through neo4j.time.DateTime; the plain
    # datetime forms pin the same structs at microsecond precision.
    assert_parameter_equals(
        driver,
        datetime(1970, 1, 1, 2, 15, 0, 42, tzinfo=timezone(timedelta(hours=1))),
        "datetime('1970-01-01T02:15:00.000042+01:00')",
        "datetime (offset, D-29)",
    )
    assert_parameter_equals(
        driver,
        DateTime(1970, 1, 1, 2, 15, 0, 42, tzinfo=pytz.FixedOffset(60)),
        "datetime('1970-01-01T02:15:00.000000042+01:00')",
        "datetime (offset, nanoseconds, D-29)",
    )
    # The zoned value uses a pytz zone, not zoneinfo.ZoneInfo: the driver
    # negotiates the Bolt 4.4 utc patch (ADR-0329, LABS-323 findings
    # F-10/F-57), and its UTC-structure encoder hands ZoneInfo a
    # neo4j.time.DateTime, which crashes or yields a wrong offset
    # nondeterministically (neo4j 6.3.1, any server that grants the
    # patch, Neo4j included).
    assert_parameter_equals(
        driver,
        pytz.timezone("Europe/Stockholm").localize(datetime(2021, 6, 1, 12, 0)),
        "datetime('2021-06-01T12:00:00+02:00[Europe/Stockholm]')",
        "datetime (zone, D-30)",
    )
    assert_parameter_equals(
        driver,
        datetime(1970, 1, 2, 0, 0, 0, 42),
        "localdatetime('1970-01-02T00:00:00.000042')",
        "localdatetime (D-31)",
    )
    assert_parameter_equals(
        driver,
        DateTime(1970, 1, 2, 0, 0, 0, 42),
        "localdatetime('1970-01-02T00:00:00.000000042')",
        "localdatetime (nanoseconds, D-31)",
    )
    assert_parameter_equals(
        driver,
        Duration(months=1, days=2, seconds=3, nanoseconds=4),
        "duration('P1M2DT3.000000004S')",
        "duration (D-32)",
    )
    print("temporal parameters ok: datetime, zoned datetime, localdatetime, duration")

    # Spatial parameters 0x58/0x59 (D-33/D-34); WGS84Point is 2D with SRID 4326.
    assert_parameter_equals(
        driver,
        CartesianPoint((1.0, 2.0)),
        "point({srid: 7203, x: 1.0, y: 2.0})",
        "CartesianPoint 2D (D-33)",
    )
    assert_parameter_equals(
        driver,
        CartesianPoint((1.0, 2.0, 3.0)),
        "point({srid: 9157, x: 1.0, y: 2.0, z: 3.0})",
        "CartesianPoint 3D (D-34)",
    )
    assert_parameter_equals(
        driver,
        WGS84Point((-73.9857, 40.7484)),
        "point({srid: 4326, x: -73.9857, y: 40.7484})",
        "WGS84Point (D-33)",
    )
    print("spatial parameters ok: CartesianPoint 2D, CartesianPoint 3D, WGS84Point")

    # Results come back as the driver's native types (ADR-0325, ADR-0326;
    # LABS-323 findings F-51..F-56, F-58..F-60 / D-09..D-17): the server
    # answers temporal and point values with PackStream structures, which
    # the driver hydrates as neo4j.time / neo4j.spatial — never strings.
    records, _, _ = driver.execute_query(
        "RETURN date('2024-01-02') AS d, "
        "time('03:04:05.123456789+01:00') AS t, "
        "localtime('03:04:05.123456789') AS lt, "
        "datetime('2024-01-02T03:04:05.123456789+01:00') AS dt, "
        "datetime('2024-01-02T03:04:05[Europe/Stockholm]') AS z, "
        "localdatetime('2024-01-02T03:04:05.123456789') AS ldt, "
        "duration('P1Y2M3DT4H5M6.5S') AS dur, "
        "point({srid: 7203, x: 1.5, y: 2.5}) AS c2, "
        "point({srid: 9157, x: 1.0, y: 2.0, z: 3.0}) AS c3, "
        "point({longitude: 13, latitude: 55.6}) AS p"
    )
    row = records[0]
    assert row["d"] == Date(2024, 1, 2), row["d"]  # F-51 / D-09
    assert row["t"] == Time(3, 4, 5, 123456789, tzinfo=offset), row["t"]  # F-52 / D-10
    assert row["lt"] == Time(3, 4, 5, 123456789), row["lt"]  # F-53 / D-11
    assert isinstance(row["dt"], DateTime), row["dt"]  # F-54 / D-12
    assert row["dt"].iso_format() == "2024-01-02T03:04:05.123456789+01:00", row["dt"]
    assert row["z"].tzinfo.utcoffset(row["z"].to_native()) == timedelta(hours=1)  # F-55 / D-13
    assert str(row["z"].tzinfo) == "Europe/Stockholm", row["z"].tzinfo
    assert row["ldt"] == DateTime(2024, 1, 2, 3, 4, 5, 123456789), row["ldt"]  # F-56 / D-14
    assert row["dur"] == Duration(
        months=14, days=3, seconds=14706, nanoseconds=500000000
    ), row["dur"]  # F-58 / D-15
    assert row["c2"] == CartesianPoint((1.5, 2.5)), row["c2"]  # F-59 / D-16
    assert row["c3"] == CartesianPoint((1.0, 2.0, 3.0)), row["c3"]  # F-60 / D-17
    assert row["p"] == WGS84Point((13.0, 55.6)), row["p"]  # F-59 / D-16
    print("temporal and spatial results ok: neo4j.time and neo4j.spatial values")

    # Bytes round-trip (ADR-0328, LABS-323 findings F-64/D-25): a bytes
    # parameter comes back as bytes.
    records, _, _ = driver.execute_query(
        "RETURN $p AS x", p=bytearray(b"\x00\x01\xff")
    )
    assert records[0]["x"] == b"\x00\x01\xff", records[0]["x"]
    print("bytes round trip ok: b'\\x00\\x01\\xff'")

    driver.close()
    print("ok")


if __name__ == "__main__":
    main()
