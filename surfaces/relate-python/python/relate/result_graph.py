"""De-duplicated result-graph extraction (ADR-0214).

``Cursor.graph()`` collects every graph entity a result's rows carry and
returns them as one :class:`ResultGraph` — the shared walk behind both that
method and ``relate.viz.to_visualization_graph``.
"""

from typing import Any, Iterable, Mapping

from relate._native import Node, Path, Relationship

# Value types that recursively contain graph entities: sequences, sets,
# and any Mapping (mirrors neo4j_viz.neo4j.find_graph_entity, which walks
# lists and dicts; a non-dict Mapping is walked too, per the docstring).
_CONTAINER_TYPES = (list, tuple, set, frozenset, Mapping)


class ResultGraph:
    """The unique graph entities of one result, de-duplicated by id.

    ``nodes`` and ``relationships`` are id-ordered immutable tuples of
    :class:`relate.Node` and :class:`relate.Relationship`. A result
    with no graph entities yields an empty ``ResultGraph``.
    """

    def __init__(self, nodes: dict, relationships: dict):
        # Sort once, at construction, into immutable tuples: the id-keyed
        # dedup dicts are the collector's shape, not the wrapper's.
        self._nodes = tuple(nodes[key] for key in sorted(nodes))
        self._relationships = tuple(
            relationships[key] for key in sorted(relationships)
        )

    @property
    def nodes(self):
        """The unique ``Node`` entities, ordered by id (immutable tuple)."""
        return self._nodes

    @property
    def relationships(self):
        """The unique ``Relationship`` entities, ordered by id (immutable tuple)."""
        return self._relationships

    def __len__(self):
        return len(self._nodes) + len(self._relationships)

    def __repr__(self):
        return (
            f"ResultGraph(nodes={len(self._nodes)}, "
            f"relationships={len(self._relationships)})"
        )


def collect_graph_entities(rows: Iterable[Mapping[str, Any]]):
    """Walk result rows and collect the unique graph entities they carry.

    Every ``Node``, ``Relationship`` and ``Path`` found — as a column value
    or nested in a list, tuple, set or map — is collected; a path contributes
    all of its nodes and relationships. De-duplication is by entity id, first
    occurrence wins. Returns ``(nodes, relationships)`` id-keyed dicts.
    """
    nodes: dict = {}
    relationships: dict = {}

    def walk(value: Any) -> None:
        if isinstance(value, Node):
            nodes.setdefault(value.id, value)
        elif isinstance(value, Relationship):
            relationships.setdefault(value.id, value)
        elif isinstance(value, Path):
            # A path is just another entity source — recurse so the dedup
            # policy lives only in the Node/Relationship branches.
            for entity in (*value.nodes, *value.relationships):
                walk(entity)
        elif isinstance(value, _CONTAINER_TYPES):
            items = value.values() if isinstance(value, Mapping) else value
            for item in items:
                walk(item)

    for row in rows:
        for value in row.values():
            walk(value)
    return nodes, relationships
