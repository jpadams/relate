"""Parameter conversion (ADR-0219): uuid, bytes, tuple, maps, explicit points."""

import uuid

import relate


def main():
    conn = relate.connect(":memory:")

    # uuid.UUID crosses as its canonical lowercase hyphenated string.
    doc_id = uuid.uuid4()
    conn.execute("CREATE (d:Doc {id: $id})", {"id": doc_id})
    row = conn.execute("MATCH (d:Doc) RETURN d.id AS id").fetchone()
    assert row["id"] == str(doc_id)

    # bytes/bytearray cross as a list of byte values 0-255 (the engine has
    # no binary scalar); the PEP 249 Binary constructor is bytes.
    row = conn.execute("RETURN $b AS b", {"b": relate.Binary(b"AB")}).fetchone()
    assert row["b"] == [65, 66]

    # Tuples convert like lists.
    row = conn.execute("RETURN $t AS t", {"t": (1, "two")}).fetchone()
    assert row["t"] == [1, "two"]

    # Dicts are always maps — srid/coordinate keys are NOT reinterpreted.
    coords = {"srid": 4326, "longitude": -73.98, "latitude": 40.75}
    row = conn.execute("RETURN $c AS c", {"c": coords}).fetchone()
    assert row["c"] == coords

    # Points are explicit: relate.Point, or point($map) in Cypher.
    row = conn.execute("RETURN point($c) AS p", {"c": {"x": 1.0, "y": 2.0}}).fetchone()
    assert isinstance(row["p"], relate.Point)
    back = conn.execute("RETURN $p AS p", {"p": row["p"]}).fetchone()["p"]
    assert isinstance(back, relate.Point)

    # Parameter-shape errors are relate.ProgrammingError with actionable
    # wording (non-dict params, non-string keys).
    for bad in ([1, 2], {42: "x"}):
        try:
            conn.execute("RETURN 1", bad)
            raise AssertionError("expected ProgrammingError")
        except relate.ProgrammingError:
            pass

    conn.close()


if __name__ == "__main__":
    main()
