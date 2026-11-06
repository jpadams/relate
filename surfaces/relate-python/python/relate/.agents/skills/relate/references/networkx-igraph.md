# Converting results to networkx / igraph

`cursor.graph()` returns a `ResultGraph` with everything the conversion
needs — entity ids, labels, type, properties, and relationship endpoint
ids — so no bridge dependency is required.

## networkx

```python
import networkx as nx

g = cur.graph()
G = nx.MultiDiGraph()
G.add_nodes_from((n.id, {"labels": n.labels, **n.properties}) for n in g.nodes)
G.add_edges_from(
    (r.start_node, r.end_node, {"type": r.type, **r.properties})
    for r in g.relationships
)
```

`MultiDiGraph` keeps parallel edges and direction. networkx auto-creates
bare nodes for endpoint ids that were not returned as columns.

## igraph

igraph indexes vertices; map ids to positions first. Endpoint ids that
were not returned as columns (e.g. `MATCH ()-[r]->() RETURN r`) must be
added as bare vertices:

```python
import igraph as ig

g = cur.graph()
ids = {n.id for n in g.nodes} | {
    e for r in g.relationships for e in (r.start_node, r.end_node)
}
index = {node_id: i for i, node_id in enumerate(sorted(ids))}
IG = ig.Graph(
    n=len(ids),
    edges=[(index[r.start_node], index[r.end_node]) for r in g.relationships],
    directed=True,
)
```

Per-attribute columns align with the vertex order:

```python
IG.vs["labels"] = [n.labels for n in g.nodes]  # only returned nodes carry these
```

Heterogeneous property sets need `None` padding:
`[n.properties.get(key) for n in g.nodes]`.

## Tabular results

Scalar-shaped results (no entity columns) go through Arrow instead:
`cur.to_arrow()` (optional `[arrow]` extra) →
`nx.from_pandas_edgelist(cur.to_df(), ...)` for edge-list-shaped frames.
