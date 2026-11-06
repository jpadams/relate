# Relate Python

Embeddable graph database with Cypher. Native PyO3 extension over
`relate::Database`.

```python
import relate

conn = relate.connect(":memory:")
conn.execute("CREATE (:Person {name: 'Alice'})")
print(list(conn.execute("MATCH (p:Person) RETURN p.name AS name")))
```

Install: `pip install relate-db` (when published; import stays
`relate`) or `maturin develop` from `surfaces/relate-python` in a source
checkout. The PyPI name `relate` is already taken by another project.
