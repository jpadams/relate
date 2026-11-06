"""Result graph extraction (ADR-0214): cursor.graph() and ResultGraph.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/result_graph.py
"""

import relate
from relate import ResultGraph


def main():
    db = relate.connect(":memory:")
    db.execute("CREATE (a:Person {name: 'Alice'})-[:KNOWS]->(b:Person {name: 'Bob'})")
    db.execute("MATCH (a:Person {name: 'Alice'}) "
               "CREATE (c:Person {name: 'Carol'})-[:KNOWS]->(a)")

    cur = db.execute("MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a, r, b")
    graph = cur.graph()
    assert isinstance(graph, ResultGraph)
    assert len(graph.nodes) == 3            # Alice, Bob, Carol — once each
    assert len(graph.relationships) == 2    # both KNOWS relationships
    assert len(graph) == 5

    # Nodes and relationships carry ids, labels/type, properties — and the
    # driver-parity members (element_id, items()) neo4j-viz-style consumers
    # read.
    node = graph.nodes[0]
    print(node.id, node.labels, dict(node.items()))
    rel = graph.relationships[0]
    print(rel.element_id, rel.type, rel.start_node, "->", rel.end_node)

    # A result with no graph entities yields an empty ResultGraph.
    empty = db.execute("MATCH (p:Person) RETURN p.name").graph()
    assert len(empty) == 0
    print(f"extracted {len(graph)} entities; empty result -> {empty!r}")


if __name__ == "__main__":
    main()
