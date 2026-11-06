"""Vectors: a typed class at every depth (ADR-0502 §1) — never a dict,
round-tripping as a parameter, hashing consistently with equality."""

import pytest

import relate


def test_vector_result_is_typed(db):
    v = db.execute("RETURN vector([1, 2, 3], 3, INTEGER8) AS v").fetchone()["v"]
    assert isinstance(v, relate.Vector)
    assert v.coordinate_type == "INTEGER8"
    assert v.dimension == 3 and len(v) == 3
    assert v.values == [1, 2, 3]
    assert str(v).startswith("vector([1, 2, 3], 3, INTEGER8")


def test_vector_float_values_and_nesting(db):
    row = db.execute(
        "WITH vector([0.5, 1.5], 2, FLOAT32) AS v "
        "RETURN [v] AS l, {inner: {v: v}} AS m"
    ).fetchone()
    (inner,) = row["l"]
    assert isinstance(inner, relate.Vector)
    assert inner.coordinate_type == "FLOAT32" and inner.values == [0.5, 1.5]
    assert isinstance(row["m"]["inner"]["v"], relate.Vector)
    # A vector is never a dict.
    assert not isinstance(row["m"]["inner"]["v"], dict)


def test_vector_property_and_param_roundtrip(db):
    db.execute("CREATE (:V {e: vector([1.0, 2.0, 3.0], 3, FLOAT32)})")
    v = db.execute("MATCH (n:V) RETURN n").fetchone()["n"]["e"]
    assert isinstance(v, relate.Vector)
    back = db.execute("RETURN $v AS v", {"v": v}).fetchone()["v"]
    assert back == v
    assert db.execute(
        "MATCH (n:V) WHERE n.e = $v RETURN count(n) AS c", {"v": v}
    ).fetchone()["c"] == 1


def test_equal_vectors_hash_equal_and_key_dicts(db):
    row = db.execute(
        "RETURN vector([1.0, 2.0], 2, FLOAT32) AS a, "
        "vector([1.0, 2.0], 2, FLOAT32) AS b, "
        "vector([0.0, 2.0], 2, FLOAT32) AS z, "
        "vector([-0.0, 2.0], 2, FLOAT32) AS nz, "
        "vector([1, 2], 2, INTEGER8) AS i"
    ).fetchone()
    a, b = row["a"], row["b"]
    assert a is not b and a == b
    assert hash(a) == hash(b)
    assert {a: "x"}[b] == "x"
    assert len({a, b}) == 1
    # -0.0 == 0.0, so the two vectors are equal and must hash equal.
    assert row["z"] == row["nz"] and hash(row["z"]) == hash(row["nz"])
    # A different coordinate type is a different key.
    assert row["i"] != a
    assert len({a, row["i"]}) == 2


def test_vector_has_no_python_constructor():
    with pytest.raises(TypeError):
        relate.Vector()
