"""Relate Python demo — build a small social graph and query it.

The `relate` package embeds the Relate engine as a native extension
module (no subprocess, no server). It is not yet published to PyPI; build it
from a checkout of the repository:

    cd surfaces/relate-python
    pip install maturin
    maturin develop --release

This demo shows both API styles:
  1. The sqlite3-style API: relate.connect() -> Connection -> Cursor
  2. The native Database class, including Arrow IPC via execute_arrow()
"""

import relate

# =====================================================================
# Part 1: sqlite3-style API (connect / Connection / Cursor)
# =====================================================================

# --- Connect to an in-memory graph database ---
db = relate.connect(":memory:")

# --- Create people ---
people = [
    {"name": "Alice", "age": 32, "city": "Portland"},
    {"name": "Bob", "age": 28, "city": "Seattle"},
    {"name": "Charlie", "age": 35, "city": "Portland"},
    {"name": "Diana", "age": 26, "city": "San Francisco"},
    {"name": "Eve", "age": 30, "city": "Seattle"},
]

for person in people:
    db.execute(
        "CREATE (:Person {name: $name, age: $age, city: $city})",
        person,
    )

# --- Create friendships ---
friendships = [
    ("Alice", "Bob"),
    ("Alice", "Charlie"),
    ("Bob", "Diana"),
    ("Charlie", "Eve"),
    ("Diana", "Eve"),
]

for a, b in friendships:
    db.execute(
        "MATCH (a:Person {name: $a}), (b:Person {name: $b}) "
        "CREATE (a)-[:FRIENDS_WITH]->(b)",
        {"a": a, "b": b},
    )

# --- Query: list everyone and their city ---
print("=== All people ===")
cursor = db.execute(
    "MATCH (p:Person) RETURN p.name AS name, p.city AS city ORDER BY p.name"
)
for row in cursor:
    print(f"  {row['name']} — {row['city']}")

# --- Query: find friends of Alice ---
print("\n=== Alice's friends ===")
cursor = db.execute(
    "MATCH (a:Person {name: 'Alice'})-[:FRIENDS_WITH]->(friend) "
    "RETURN friend.name AS name ORDER BY name"
)
for row in cursor:
    print(f"  {row['name']}")

# --- Query: friends-of-friends (2 hops) ---
print("\n=== Alice's friends-of-friends ===")
cursor = db.execute(
    "MATCH (a:Person {name: 'Alice'})-[:FRIENDS_WITH]->(mid)-[:FRIENDS_WITH]->(fof) "
    "RETURN DISTINCT fof.name AS name ORDER BY name"
)
for row in cursor:
    print(f"  {row['name']}")

# --- Query: people in each city (aggregation) ---
print("\n=== People per city ===")
cursor = db.execute(
    "MATCH (p:Person) "
    "RETURN p.city AS city, count(p) AS total, collect(p.name) AS names "
    "ORDER BY total DESC"
)
for row in cursor:
    names = ", ".join(row["names"])
    print(f"  {row['city']}: {row['total']} ({names})")

# --- Parameterized query: filter by minimum age ---
min_age = 30
print(f"\n=== People aged {min_age}+ ===")
cursor = db.execute(
    "MATCH (p:Person) WHERE p.age >= $min_age "
    "RETURN p.name AS name, p.age AS age ORDER BY p.age DESC",
    {"min_age": min_age},
)
for row in cursor:
    print(f"  {row['name']} (age {row['age']})")

# --- Mutation stats ---
print("\n=== Mutation stats ===")
cursor = db.execute("CREATE (:Project {name: 'Relate', lang: 'Rust'})")
stats = cursor.stats
print(f"  Nodes created: {stats['nodes_created']}")
print(f"  Properties set: {stats['properties_set']}")

# --- Clean up ---
db.close()

# =====================================================================
# Part 2: native Database class + Arrow IPC
# =====================================================================

from relate import Database

ndb = Database()  # in-memory; pass a file path for persistence

for person in people:
    ndb.execute(
        "CREATE (:Person {name: $name, age: $age, city: $city})",
        person,
    )

print("\n=== Native Database introspection ===")
print(f"  Database: {ndb}")
print(f"  Node count: {ndb.node_count()}")
print(f"  Labels: {ndb.labels()}")

result = ndb.execute(
    "MATCH (p:Person) RETURN p.name AS name, p.age AS age ORDER BY p.name"
)
print(f"  Columns: {result.columns}")
print(f"  First row: {result.data[0]}")

# --- Arrow IPC: zero-copy transfer to PyArrow / Pandas ---
print("\n=== Arrow IPC (requires pyarrow) ===")
try:
    import pyarrow as pa

    buf = ndb.execute_arrow(
        "MATCH (p:Person) RETURN p.name AS name, p.age AS age ORDER BY p.name"
    )
    table = pa.ipc.open_stream(buf).read_all()
    df = table.to_pandas()
    print(df.to_string(index=False))
except ImportError:
    print("  (skipped — install pyarrow: pip install pyarrow)")

ndb.close()
print("\nDone.")
