# Example: official Python driver

```python
from neo4j import GraphDatabase

driver = GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "unused"))
records, _, _ = driver.execute_query("RETURN 1 AS n")
assert records[0]["n"] == 1
with driver.session() as session:
    with session.begin_transaction() as tx:
        tx.run("CREATE (n:Person {name: $name})", name="Ada")
        name = tx.run("MATCH (n:Person) RETURN n.name AS name").single()["name"]
        assert name == "Ada"
        tx.commit()
driver.close()
```

Start the server first: `cargo run -p relate-cli -- serve --host 127.0.0.1 --bolt 7687 --database :memory:` (ADR-0368).
