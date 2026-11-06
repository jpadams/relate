"""Rich value objects returned from queries: Node / Relationship / Path / Point,
and temporal / point parameter round-trips (both directions)."""

import datetime

import pytest

import relate


# ============================================================
# Node
# ============================================================


def test_node_is_node_instance(db):
    db.execute("CREATE (:Person {name: 'Alice', age: 30})")
    cur = db.execute("MATCH (n:Person) RETURN n")
    node = cur.fetchone()["n"]
    assert isinstance(node, relate.Node)


def test_node_exposes_id(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    node = db.execute("MATCH (n:Person) RETURN n").fetchone()["n"]
    assert isinstance(node.id, int)


def test_node_exposes_labels(db):
    db.execute("CREATE (:Person:Employee {name: 'Alice'})")
    node = db.execute("MATCH (n:Person) RETURN n").fetchone()["n"]
    assert node.labels == ["Person", "Employee"]


def test_node_properties_are_ordered_dict(db):
    db.execute("CREATE (:Person {name: 'Alice', age: 30, city: 'NYC'})")
    node = db.execute("MATCH (n:Person) RETURN n").fetchone()["n"]
    assert node.properties["name"] == "Alice"
    assert node.properties["age"] == 30
    # Relate canonicalizes stored property keys while preserving deterministic
    # iteration order in the rich Python mapping.
    assert list(node.properties.keys()) == ["age", "city", "name"]


def test_node_getitem_shorthand(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    node = db.execute("MATCH (n:Person) RETURN n").fetchone()["n"]
    assert node["name"] == "Alice"
    assert "name" in node
    with pytest.raises(KeyError):
        _ = node["nonexistent"]


def test_node_repr(db):
    db.execute("CREATE (:Person {name: 'Alice'})")
    node = db.execute("MATCH (n:Person) RETURN n").fetchone()["n"]
    assert "Node(" in repr(node)
    assert "Person" in repr(node)


# ============================================================
# Relationship (topology is recoverable)
# ============================================================


def test_relationship_is_relationship_instance(db):
    db.execute("CREATE (:A)-[:KNOWS]->(:B)")
    rel = db.execute("MATCH ()-[r:KNOWS]->() RETURN r").fetchone()["r"]
    assert isinstance(rel, relate.Relationship)


def test_relationship_type_and_properties(db):
    db.execute("CREATE (:A)-[:KNOWS {since: 2020}]->(:B)")
    rel = db.execute("MATCH ()-[r:KNOWS]->() RETURN r").fetchone()["r"]
    assert rel.type == "KNOWS"
    assert rel.properties["since"] == 2020


def test_relationship_endpoints_match_node_ids(db):
    """The relationship's start_node/end_node ids let the caller rebuild the
    topology from Node.id -- the whole point of returning real objects."""
    db.execute("CREATE (:A {name: 'a'})-[:KNOWS]->(:B {name: 'b'})")
    row = db.execute("MATCH (a)-[r:KNOWS]->(b) RETURN a, r, b").fetchone()
    a, r, b = row["a"], row["r"], row["b"]
    assert r.start_node == a.id
    assert r.end_node == b.id
    assert a.id != b.id


# ============================================================
# Path
# ============================================================


def test_path_object(db):
    db.execute("CREATE (:A)-[:R]->(:B)-[:R]->(:C)")
    cur = db.execute("MATCH p = (:A)-[:R*2]->(:C) RETURN p")
    path = cur.fetchone()["p"]
    assert isinstance(path, relate.Path)
    assert len(path.nodes) == 3
    assert len(path.relationships) == 2
    assert len(path) == 2  # number of relationships


def test_path_nodes_are_nodes(db):
    db.execute("CREATE (:A)-[:R]->(:B)")
    path = db.execute("MATCH p = (:A)-[:R]->(:B) RETURN p").fetchone()["p"]
    assert all(isinstance(n, relate.Node) for n in path.nodes)
    assert all(isinstance(r, relate.Relationship) for r in path.relationships)
    # Topology chains: each rel connects consecutive nodes.
    assert path.relationships[0].start_node == path.nodes[0].id
    assert path.relationships[0].end_node == path.nodes[1].id


# ============================================================
# Point (round-trips both directions)
# ============================================================


def test_point_return_is_point(db):
    p = db.execute("RETURN point({x: 1.0, y: 2.0}) AS p").fetchone()["p"]
    assert isinstance(p, relate.Point)
    assert p.x == 1.0
    assert p.y == 2.0


def test_geographic_point_aliases(db):
    p = db.execute(
        "RETURN point({longitude: -73.98, latitude: 40.75}) AS p"
    ).fetchone()["p"]
    assert p.crs == "wgs-84"
    assert abs(p.longitude - (-73.98)) < 1e-9
    assert abs(p.latitude - 40.75) < 1e-9


def test_point_param_roundtrip(db):
    p = relate.Point(4326, -73.98, 40.75)
    back = db.execute("RETURN $p AS p", {"p": p}).fetchone()["p"]
    assert isinstance(back, relate.Point)
    assert back.srid == 4326
    assert abs(back.x - (-73.98)) < 1e-9
    assert abs(back.y - 40.75) < 1e-9


def test_point_3d(db):
    p = db.execute("RETURN point({x: 1.0, y: 2.0, z: 3.0}) AS p").fetchone()["p"]
    assert p.z == 3.0


# ============================================================
# Temporal parameters (both directions)
# ============================================================


def test_date_param_roundtrip(db):
    d = datetime.date(2024, 3, 14)
    back = db.execute("RETURN $d AS d", {"d": d}).fetchone()["d"]
    assert isinstance(back, datetime.date)
    assert back == d


def test_naive_datetime_roundtrip(db):
    dt = datetime.datetime(2024, 3, 14, 12, 30, 45)
    back = db.execute("RETURN $dt AS d", {"dt": dt}).fetchone()["d"]
    assert isinstance(back, datetime.datetime)
    assert back.tzinfo is None
    assert back == dt


def test_aware_datetime_roundtrip(db):
    tz = datetime.timezone(datetime.timedelta(hours=2))
    dt = datetime.datetime(2024, 3, 14, 12, 30, 45, tzinfo=tz)
    back = db.execute("RETURN $dt AS d", {"dt": dt}).fetchone()["d"]
    assert isinstance(back, datetime.datetime)
    assert back.tzinfo is not None
    assert back == dt


class _HugeOffsetDatetime(datetime.datetime):
    """Overrides utcoffset() with a value the stdlib would never return."""

    def utcoffset(self):
        return datetime.timedelta(days=999_999_999)


class _HugeOffsetTime(datetime.time):
    def utcoffset(self):
        return datetime.timedelta(days=999_999_999)


class _ShadowedTimedelta(datetime.timedelta):
    """A timedelta whose ``days`` attribute lies (a subclass shadow)."""

    @property
    def days(self):
        return 10**15


class _ShadowingTz(datetime.tzinfo):
    def utcoffset(self, dt):
        return _ShadowedTimedelta(hours=2)

    def dst(self, dt):
        return None


def test_datetime_utcoffset_override_is_ignored(db):
    """A subclass's utcoffset() override never reaches the conversion: the
    stdlib method answers (naive here), so a huge offset cannot wrap."""
    dt = _HugeOffsetDatetime(2024, 3, 14, 12, 30, 45)
    back = db.execute("RETURN $dt AS d", {"dt": dt}).fetchone()["d"]
    assert back.tzinfo is None
    assert back == datetime.datetime(2024, 3, 14, 12, 30, 45)


def test_time_utcoffset_override_is_ignored(db):
    t = _HugeOffsetTime(12, 30, 45)
    back = db.execute("RETURN $t AS t", {"t": t}).fetchone()["t"]
    assert back.tzinfo is None


def test_offset_timedelta_shadowed_days_is_ignored(db):
    """The offset's components are read through the base timedelta, so a
    shadowed ``days`` cannot overflow the offset arithmetic."""
    dt = datetime.datetime(2024, 3, 14, 12, 30, 45, tzinfo=_ShadowingTz())
    back = db.execute("RETURN $dt AS d", {"dt": dt}).fetchone()["d"]
    assert back.utcoffset() == datetime.timedelta(hours=2)


def test_huge_tzinfo_offset_is_rejected(db):
    """A tzinfo answering an out-of-range offset raises a typed error from
    the stdlib's own validation instead of wrapping into a wrong offset."""

    class HugeTz(datetime.tzinfo):
        def utcoffset(self, dt):
            return datetime.timedelta(days=999_999_999)

        def dst(self, dt):
            return None

    dt = datetime.datetime(2024, 3, 14, 12, 30, 45, tzinfo=HugeTz())
    with pytest.raises(ValueError, match="strictly between"):
        db.execute("RETURN $dt AS d", {"dt": dt})


def test_time_param_roundtrip(db):
    t = datetime.time(12, 30, 45)
    back = db.execute("RETURN $t AS t", {"t": t}).fetchone()["t"]
    assert isinstance(back, datetime.time)
    assert back.hour == 12 and back.minute == 30 and back.second == 45


def test_timedelta_param_roundtrip(db):
    td = datetime.timedelta(days=1, hours=2, minutes=3)
    back = db.execute("RETURN $td AS d", {"td": td}).fetchone()["d"]
    # Durations come back as the lossless Duration (ADR-0502 §1); the
    # timedelta is its explicit projection.
    assert isinstance(back, relate.Duration)
    assert back.to_timedelta() == td


def test_datetime_microseconds_preserved(db):
    dt = datetime.datetime(2024, 3, 14, 12, 30, 45, 123456)
    back = db.execute("RETURN $dt AS d", {"dt": dt}).fetchone()["d"]
    assert back.microsecond == 123456
