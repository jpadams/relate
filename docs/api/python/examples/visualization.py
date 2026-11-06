"""Visualizing Relate results with neo4j-viz.

Run from the repo root after `pip install ./surfaces/relate-python`
and `pip install neo4j-viz`:

    python docs/api/python/examples/visualization.py

The example builds the graph headlessly: `to_visualization_graph` returns
the VisualizationGraph, whose `render_widget()` (Jupyter) / `render()`
(standalone HTML) call is what needs a frontend, so it is only asserted on
here.
"""

import relate
from relate.viz import to_visualization_graph


def main():
    db = relate.connect(":memory:")
    db.execute(
        """
        CREATE (a:Tag {name: 'neo4j'})
        CREATE (b:Tag {name: 'cypher'})
        CREATE (q:Question {title: 'How?'})-[:TAGGED]->(a)-[:SIMILAR_TO {jaccard: 0.75}]->(b)
        """
    )

    # One query, returned graph elements: nodes, relationships and paths
    # anywhere in the rows (a column, or nested in a list/map) land in the
    # visualization. Captions default to the node labels and the
    # relationship type; every property is kept, plus `labels`/`type`.
    vg = to_visualization_graph(
        db.execute("MATCH (a:Tag)-[s:SIMILAR_TO]->(b:Tag) RETURN a, s, b")
    )

    assert len(vg.nodes) == 2
    # captions default to the first name-ish property (name > title >
    # display_name), falling back to the labels; per-label mapping via
    # node_caption={"Question": "title", ...}
    assert {n.caption for n in vg.nodes} == {"neo4j", "cypher"}
    assert {n.properties["name"] for n in vg.nodes} == {"neo4j", "cypher"}
    assert len(vg.relationships) == 1
    rel = vg.relationships[0]
    assert rel.caption == "SIMILAR_TO"
    assert rel.properties["jaccard"] == 0.75
    node_ids = {n.id for n in vg.nodes}
    assert rel.source in node_ids and rel.target in node_ids

    # Display in a frontend:
    #   vg.render_widget()   # JupyterLab / Notebook 7 / VS Code / Colab
    #   vg.render()          # standalone HTML
    # (nodes are already colored by label with the Neo4j palette)

    # A result without graph entities is a clear error.
    try:
        to_visualization_graph(db.execute("MATCH (t:Tag) RETURN t.name AS name"))
    except ValueError as e:
        assert "no graph entities" in str(e)
    else:  # pragma: no cover
        raise AssertionError("expected ValueError for a scalar-only result")

    db.close()
    print("visualization example OK")


if __name__ == "__main__":
    main()
