"""Tests for relate.viz (optional neo4j-viz integration).

The whole module skips when neo4j-viz is not installed: it is an optional
dependency, and the helper degrades to a helpful ImportError without it.
The one behavior that must hold without neo4j-viz — that error message —
is covered separately in ``test_no_viz_library``.
"""

import re
import sys

import pytest

pytest.importorskip("neo4j_viz")

import relate  # noqa: E402
from relate.viz import render_html, to_visualization_graph  # noqa: E402


@pytest.fixture()
def db():
    conn = relate.connect(":memory:")
    conn.execute(
        """
        CREATE (a:Tag {name: 'neo4j'})-[:SIMILAR_TO {jaccard: 0.75}]->(b:Tag {name: 'cypher'})
        CREATE (q:Question {title: 'How?'})-[:TAGGED]->(a)
        """
    )
    yield conn
    conn.close()


def test_nodes_and_relationships_from_columns(db):
    vg = to_visualization_graph(
        db.execute("MATCH (a:Tag)-[s:SIMILAR_TO]->(b:Tag) RETURN a, s, b")
    )
    assert {n.caption for n in vg.nodes} == {"neo4j", "cypher"}
    assert len(vg.nodes) == 2
    assert {n.properties["name"] for n in vg.nodes} == {"neo4j", "cypher"}
    assert all(n.properties["labels"] == ["Tag"] for n in vg.nodes)
    assert len(vg.relationships) == 1
    rel = vg.relationships[0]
    assert rel.caption == "SIMILAR_TO"
    assert rel.properties["type"] == "SIMILAR_TO"
    assert rel.properties["jaccard"] == 0.75


def test_relationship_endpoints_are_node_ids(db):
    vg = to_visualization_graph(db.execute("MATCH (a:Tag)-[s]->(b:Tag) RETURN a, s, b"))
    node_ids = {n.id for n in vg.nodes}
    assert vg.relationships[0].source in node_ids
    assert vg.relationships[0].target in node_ids


def test_nodes_colored_by_labels(db):
    db.execute("CREATE (p:Person {name: 'Alice'})")
    vg = to_visualization_graph(db.execute("MATCH (n) RETURN n"))
    colored = {tuple(n.properties["labels"]): n.color for n in vg.nodes}
    assert set(colored) == {("Tag",), ("Question",), ("Person",)}
    assert len(set(colored.values())) == 3  # one color per label


def test_caption_defaults_to_the_first_nameish_property(db):
    db.execute("CREATE (p:Person {name: 'Alice'}) CREATE (q:Q {title: 'T'}) CREATE (u:U {display_name: 'D'})")
    vg = to_visualization_graph(db.execute("MATCH (n) WHERE n:Person OR n:Q OR n:U RETURN n"))
    captions = sorted(n.caption for n in vg.nodes)
    assert captions == ["Alice", "D", "T"]


def test_caption_falls_back_to_labels(db):
    vg = to_visualization_graph(db.execute("MATCH (q:Question)-[r:TAGGED]->(t:Tag) RETURN r"))
    assert vg.nodes == []  # relationship-only result
    assert vg.relationships[0].caption == "TAGGED"
    # a node with no name-ish property is captioned by its labels
    db.execute("CREATE (w:Widget {serial: 7})")
    vg2 = to_visualization_graph(db.execute("MATCH (w:Widget) RETURN w"))
    assert vg2.nodes[0].caption == "Widget"


def test_node_caption_map_overrides_and_stringifies(db):
    vg = to_visualization_graph(
        db.execute("MATCH (q:Question) RETURN q"),
        node_caption={"Question": "is_accepted"},
    )
    assert vg.nodes[0].caption == "How?"  # missing property falls back to name-ish
    vg2 = to_visualization_graph(
        db.execute("MATCH (q:Question) RETURN q"),
        node_caption={"Question": "title"},
    )
    assert vg2.nodes[0].caption == "How?"
    db.execute("CREATE (a:Answer {is_accepted: true})")
    vg3 = to_visualization_graph(
        db.execute("MATCH (a:Answer) RETURN a"),
        node_caption={"Answer": "is_accepted"},
    )
    assert vg3.nodes[0].caption == "True"  # non-string values are stringified


def test_paths_contribute_nodes_and_relationships(db):
    vg = to_visualization_graph(
        db.execute("MATCH p=(q:Question)-[r]->(t:Tag) RETURN p")
    )
    assert len(vg.relationships) >= 1
    assert {n.properties["labels"][0] for n in vg.nodes} >= {"Question", "Tag"}


def test_entities_nested_in_lists_and_maps(db):
    vg = to_visualization_graph(
        db.execute("MATCH (a:Tag) RETURN [a, {inner: a}] AS bundle")
    )
    assert len(vg.nodes) == 2  # deduped by id


def test_works_with_native_query_result_and_plain_rows():
    ndb = relate.Database(":memory:")
    ndb.execute("CREATE (a:Tag)-[r:LINK]->(b:Tag)")
    # native QueryResult
    vg = to_visualization_graph(ndb.execute("MATCH (a:Tag)-[r:LINK]->(b:Tag) RETURN a, r, b"))
    assert len(vg.nodes) == 2 and len(vg.relationships) == 1
    # plain list of dict rows
    vg2 = to_visualization_graph(ndb.execute("MATCH (t:Tag) RETURN t").data)
    assert len(vg2.nodes) == 2


def test_scalar_only_result_is_a_value_error(db):
    with pytest.raises(ValueError, match="no graph entities"):
        to_visualization_graph(db.execute("MATCH (t:Tag) RETURN t.name AS name"))


def test_render_html_uniquifies_the_data_block():
    """Two renders on one page must not share the page-global data id."""
    ndb = relate.Database(":memory:")
    # distinctive names: the word must not appear in the bundled template JS
    ndb.execute("CREATE (a:Tag {name: 'alfa-bruno'}) CREATE (b:Tag {name: 'charlie-delta'})")
    vg_a = to_visualization_graph(ndb.execute("MATCH (t:Tag {name: 'alfa-bruno'}) RETURN t"))
    vg_b = to_visualization_graph(ndb.execute("MATCH (t:Tag {name: 'charlie-delta'}) RETURN t"))
    html_a = render_html(vg_a).data
    html_b = render_html(vg_b).data

    assert 'id="neo4j-viz-data"' not in html_a  # the page-global id is renamed
    assert 'getElementById(`neo4j-viz-data' in html_a  # lookup renamed with it
    ids_a = {m for m in re.findall(r"neo4j-viz-data-[0-9a-f]{12}", html_a)}
    ids_b = {m for m in re.findall(r"neo4j-viz-data-[0-9a-f]{12}", html_b)}
    assert len(ids_a) == 1 and len(ids_b) == 1  # one id, renamed everywhere
    assert ids_a != ids_b  # distinct per render
    assert "alfa-bruno" in html_a and "charlie-delta" not in html_a  # own data only
    assert "charlie-delta" in html_b and "alfa-bruno" not in html_b


def test_documented_example_runs():
    """The docs example is part of the contract; execute it for real."""
    import runpy
    from pathlib import Path

    runpy.run_path(
        str(
            Path(__file__).resolve().parents[4]
            / "docs"
            / "api"
            / "python"
            / "examples"
            / "visualization.py"
        ),
        run_name="__main__",
    )


def test_no_viz_library(monkeypatch):
    """Without neo4j-viz the helper explains how to install it."""
    monkeypatch.setitem(sys.modules, "neo4j_viz", None)
    import relate.viz as viz

    db = relate.connect(":memory:")
    db.execute("CREATE (t:Tag)")
    with pytest.raises(ImportError, match="neo4j-viz"):
        viz.to_visualization_graph(db.execute("MATCH (t:Tag) RETURN t"))
    db.close()
