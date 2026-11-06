"""Parameter conversion coverage (ADR-0219): uuid, bytes/bytearray, tuple,
explicit-only points, and DB-API-worded parameter errors."""

import uuid

import pytest

import relate


def test_uuid_param_becomes_canonical_string(db):
    u = uuid.UUID("12345678-1234-5678-9abc-def012345678")
    back = db.execute("RETURN $u AS u", {"u": u}).fetchone()["u"]
    assert back == "12345678-1234-5678-9abc-def012345678"


def test_uuid_param_normalizes_case(db):
    u = uuid.UUID("12345678-1234-5678-9ABC-DEF012345678")
    back = db.execute("RETURN $u AS u", {"u": u}).fetchone()["u"]
    assert back == "12345678-1234-5678-9abc-def012345678"


def test_uuid_param_usable_in_write(db):
    db.execute("CREATE (n:Doc {id: $id})", {"id": uuid.uuid4()})
    row = db.execute("MATCH (n:Doc) RETURN n.id AS id").fetchone()
    assert isinstance(row["id"], str)
    uuid.UUID(row["id"])  # parses back


def test_bytes_param_becomes_int_list(db):
    back = db.execute("RETURN $b AS b", {"b": b"\x00\x01\xff"}).fetchone()["b"]
    assert back == [0, 1, 255]


def test_bytearray_param_becomes_int_list(db):
    back = db.execute("RETURN $b AS b", {"b": bytearray(b"AB")}).fetchone()["b"]
    assert back == [65, 66]


def test_dbapi_binary_constructor_is_accepted(db):
    # PEP 249: Binary() constructs the value bound for binary columns.
    back = db.execute("RETURN $b AS b", {"b": relate.Binary(b"xy")}).fetchone()["b"]
    assert back == [120, 121]


def test_tuple_param_becomes_list(db):
    back = db.execute("RETURN $t AS t", {"t": (1, "two", 3.5)}).fetchone()["t"]
    assert back == [1, "two", 3.5]


def test_nested_tuple_param(db):
    back = db.execute("RETURN $t AS t", {"t": ((1, 2), (3,))}).fetchone()["t"]
    assert back == [[1, 2], [3]]


def test_srid_dict_stays_a_map(db):
    # ADR-0219: dicts are always maps; no implicit point reinterpretation.
    param = {"srid": 4326, "x": 1.0, "y": 2.0}
    back = db.execute("RETURN $m AS m", {"m": param}).fetchone()["m"]
    assert isinstance(back, dict)
    assert back == param


def test_lonlat_srid_dict_stays_a_map(db):
    # Same ADR-0219 policy as test_srid_dict_stays_a_map, lon/lat keys.
    param = {"srid": 4326, "longitude": -73.98, "latitude": 40.75}
    back = db.execute("RETURN $m AS m", {"m": param}).fetchone()["m"]
    assert isinstance(back, dict)
    assert back == param


def test_explicit_point_param_still_roundtrips(db):
    p = relate.Point(4326, -73.98, 40.75)
    back = db.execute("RETURN $p AS p", {"p": p}).fetchone()["p"]
    assert isinstance(back, relate.Point)
    assert back.srid == 4326


def test_point_from_coordinate_map_via_cypher(db):
    # The supported way to make a point from a plain map is `point($map)`.
    p = db.execute(
        "RETURN point($coords) AS p", {"coords": {"x": 1.0, "y": 2.0}}
    ).fetchone()["p"]
    assert isinstance(p, relate.Point)
    assert p.x == 1.0 and p.y == 2.0


def test_non_dict_params_raise_programming_error(db):
    with pytest.raises(relate.ProgrammingError) as excinfo:
        db.execute("RETURN 1", [1, 2])
    assert "parameters must be a dict" in str(excinfo.value)
    assert "'list'" in str(excinfo.value)


def test_non_string_param_key_raises_programming_error(db):
    with pytest.raises(relate.ProgrammingError) as excinfo:
        db.execute("RETURN 1", {42: "x"})
    assert "keys must be strings" in str(excinfo.value)
    assert "'int'" in str(excinfo.value)


def test_non_string_nested_map_key_raises_programming_error(db):
    with pytest.raises(relate.ProgrammingError) as excinfo:
        db.execute("RETURN $m AS m", {"m": {1.5: "x"}})
    assert "keys must be strings" in str(excinfo.value)


def test_param_errors_do_not_break_the_connection(db):
    with pytest.raises(relate.ProgrammingError):
        db.execute("RETURN 1", [1, 2])
    assert db.execute("RETURN 1 AS n").fetchone()["n"] == 1
