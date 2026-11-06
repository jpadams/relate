"""Visualize Relate query results with the neo4j-viz library.

`neo4j-viz <https://neo4j.com/docs/python-graph-visualization/current/>`_
wraps the Neo4j Visualization Library for interactive graph rendering in
Python (Jupyter and beyond). It is an *optional* dependency of Relate:
:func:`to_visualization_graph` imports it lazily and raises
:class:`ImportError` with install instructions when it is missing, so the
rest of the package works without it.

The helper consumes one query result — the graph elements it contains, not
a prepared table. Any :class:`relate.Node`, :class:`relate.Relationship`
or :class:`relate.Path` a row carries (directly, or inside a list or map)
lands in the visualization::

    import relate
    from relate.viz import to_visualization_graph

    db = relate.connect(":memory:")
    vg = to_visualization_graph(
        db.execute("MATCH (a:Tag)-[s:SIMILAR_TO]->(b:Tag) RETURN a, s, b")
    )
    vg.render_widget()  # or vg.render() for standalone HTML

Following neo4j-viz's own Neo4j-driver integration (`neo4j_viz.neo4j`),
captions default to the node labels and the relationship type, every property
is kept under ``properties`` (plus a ``labels``/``type`` entry), and nodes are
colored by their labels with the Neo4j discrete palette when it has enough
colors.
"""

from typing import Any, Iterable, Mapping

from relate.result_graph import collect_graph_entities

# Node caption fallback order when no per-label mapping is given: the first
# of these properties a node carries wins; a node with none gets its labels.
_NAMEISH_KEYS = ("name", "title", "display_name")


def _node_caption(node, node_caption):
    if node_caption:
        for label in node.labels:
            key = node_caption.get(label)
            if key is not None and node.properties.get(key) is not None:
                return str(node.properties[key])
    for key in _NAMEISH_KEYS:
        if key in node.properties:
            return str(node.properties[key])
    return ":".join(node.labels)


def render_html(vg, **kwargs):
    """Standalone-HTML render whose data block is unique per call.

    neo4j-viz's standalone template ships its graph data in a
    ``<script type="application/json" id="neo4j-viz-data">`` block and reads
    it back with ``document.getElementById("neo4j-viz-data")`` — a fixed,
    page-global id. The graph container gets a unique id per render, but the
    data block does not, so two ``render()`` outputs on one page (two
    notebook cells) both paint the first cell's graph. This wrapper renames
    the id and its lookup together, per call, so each output paints its own
    graph. All keyword arguments pass through to ``vg.render()`` (e.g.
    ``initial_zoom``, ``width``, ``height``).
    """
    import uuid

    from IPython.display import HTML

    html = vg.render(**kwargs).data
    unique = f"neo4j-viz-data-{uuid.uuid4().hex[:12]}"
    return HTML(html.replace("neo4j-viz-data", unique))


def _rows_of(result: Any) -> Iterable[Mapping[str, Any]]:
    """Dict rows from a DB-API cursor, a native QueryResult, or plain rows."""
    if hasattr(result, "fetchall"):
        return result.fetchall()
    if hasattr(result, "data"):
        return result.data
    return result


def to_visualization_graph(result: Any, node_caption=None):
    """Build a ``neo4j_viz.VisualizationGraph`` from one query result.

    ``result`` is anything whose rows are mappings: a ``Cursor`` from a DB-API
    connection (``fetchall()``), a native ``QueryResult`` (``.data``), or a
    plain iterable of dicts. Every ``Node``, ``Relationship`` and ``Path``
    found in the rows — as a top-level column value or nested in a list or
    map — becomes part of the visualization; a path contributes all of its
    nodes and relationships.

    ``node_caption`` optionally maps a label to the property that captions
    its nodes — ``{"Question": "title", "Tag": "name"}``; without it, a node
    is captioned by the first of ``name`` / ``title`` / ``display_name`` it
    carries, else by its labels. Values are stringified. Relationship
    captions stay the relationship type.

    Raises :class:`ImportError` when neo4j-viz is not installed, and
    :class:`ValueError` when the result contains no graph entities at all.

    Returns the ``VisualizationGraph``; style and display it with its own
    methods (``render_widget`` in Jupyter, ``render`` for standalone HTML).
    """
    try:
        from neo4j_viz import Node as VizNode
        from neo4j_viz import Relationship as VizRelationship
        from neo4j_viz import VisualizationGraph
        from neo4j_viz.colors import NEO4J_COLORS_DISCRETE, ColorSpace
    except ImportError as exc:  # pragma: no cover - depends on environment
        raise ImportError(
            "neo4j-viz is required for visualization; "
            "install it with: pip install neo4j-viz"
        ) from exc

    nodes, relationships = collect_graph_entities(_rows_of(result))
    if not nodes and not relationships:
        raise ValueError(
            "the result contains no graph entities; return nodes, "
            "relationships or paths (e.g. MATCH (a)-[r]->(b) RETURN a, r, b)"
        )

    def map_node(node):
        properties = {**node.properties, "labels": node.labels}
        return VizNode(
            id=node.id,
            caption=_node_caption(node, node_caption),
            properties=properties,
        )

    def map_relationship(rel):
        properties = {**rel.properties, "type": rel.type}
        return VizRelationship(
            id=rel.id,
            source=rel.start_node,
            target=rel.end_node,
            caption=rel.type,
            properties=properties,
        )

    vg = VisualizationGraph(
        nodes=[map_node(n) for n in nodes.values()],
        relationships=[map_relationship(r) for r in relationships.values()],
    )
    label_sets = {tuple(n.properties["labels"]) for n in vg.nodes}
    if label_sets and len(label_sets) <= len(NEO4J_COLORS_DISCRETE):
        # Color by label, not caption: captions are per-node values (a title,
        # a name), so a caption-colored graph would give up on more than 12
        # nodes instead of giving one color per label.
        vg.color_nodes(
            property="labels", color_space=ColorSpace.DISCRETE, colors=NEO4J_COLORS_DISCRETE
        )
    return vg
