"""Engine facts the binding carries verbatim (ADR-0502).

Errors carry the engine's Neo4j code, GQLSTATUS, stage and retryable flag;
writes return bookmarks (the engine's commit sequence); durations arrive as
the lossless ``relate.Duration``; vectors as ``relate.Vector``; and
element ids are exactly what Cypher ``elementId()`` returns.
"""

import relate

conn = relate.connect(":memory:")

# --- Errors: one classification, from the engine ----------------------------
try:
    conn.execute("RETURN $missing AS m")
except relate.ProgrammingError as e:
    print(e.neo4j_code)   # Neo.ClientError.Statement.ParameterMissing
    print(e.gql_status)   # 42N02
    print(e.stage)        # compile
    print(e.retryable)    # False -- a retry cannot succeed
    assert e.neo4j_code == "Neo.ClientError.Statement.ParameterMissing"
    assert e.retryable is False

# --- Bookmarks: the commit sequence, monotonic per store ---------------------
first = conn.execute("CREATE (:Person {name: 'Ada'})").consume().bookmark
second = conn.execute("CREATE (:Person {name: 'Bob'})").consume().bookmark
print(first, second)      # relate:1 relate:2
assert int(second.split(":")[1]) > int(first.split(":")[1])
assert conn.execute("MATCH (p:Person) RETURN p").consume().bookmark is None

conn.autocommit = False
conn.execute("CREATE (:Person {name: 'Cy'})")
committed = conn.commit()  # the explicit transaction's bookmark
assert int(committed.split(":")[1]) > int(second.split(":")[1])
conn.autocommit = True

# --- Durations: months, days, seconds, nanoseconds -- never merged -----------
row = conn.execute(
    "RETURN duration('PT36H') AS a, duration('P1DT12H') AS b, "
    "duration('P1M2DT0.000000005S') AS c"
).single()
a, b, c = row["a"], row["b"], row["c"]
print(repr(a))            # Duration(months=0, days=0, seconds=129600, nanoseconds=0)
assert a != b             # Cypher keeps 36 hours apart from 1 day 12 hours
assert (c.months, c.days, c.nanoseconds) == (1, 2, 5)
print(str(c))             # P1M2DT0.000000005S
back = conn.execute("RETURN $d AS d", {"d": c}).single()["d"]
assert back == c          # binds back without loss
print(a.to_timedelta())   # 1 day, 12:00:00 -- the explicit lossy projection

# --- Vectors: a typed class at every depth -----------------------------------
v = conn.execute("RETURN [vector([1.0, 2.0, 3.0], 3, FLOAT32)] AS vs").single()["vs"][0]
print(v.coordinate_type, v.dimension, v.values)  # FLOAT32 3 [1.0, 2.0, 3.0]
assert isinstance(v, relate.Vector)

# --- Element ids: Cypher's elementId() spelling ------------------------------
conn.execute("MATCH (a {name: 'Ada'}), (b {name: 'Bob'}) CREATE (a)-[:KNOWS]->(b)")
rel = conn.execute("MATCH ()-[r:KNOWS]->() RETURN r").single()["r"]
print(rel.element_id, rel.start_node_element_id, rel.end_node_element_id)
found = conn.execute(
    "MATCH (n) WHERE elementId(n) = $id RETURN n.name AS name",
    {"id": rel.start_node_element_id},
).single()
assert found["name"] == "Ada"

conn.close()
