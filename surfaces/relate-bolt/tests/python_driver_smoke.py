#!/usr/bin/env python3
"""Official Neo4j driver smoke test against `relate serve`'s Bolt surface.

The current python driver (verified 5.23 through 5.28) validates the HELLO
`server` agent and rejects anything not starting `Neo4j/`, so this smoke
connects to servers started behind the `--bolt-identity` compat shim
(ADR-0293) — scripts/bolt-smoke.sh owns that topology and exports it:
  RELATE_BOLT_PORT_IDENTITY       the shimmed plaintext server
  RELATE_BOLT_PORT_AUTH_IDENTITY  the shimmed preset-auth server
Run through the script (or export the variables beside your own
servers started with `--bolt-identity Neo4j/5.26.0`); every topology
fact is required — unset is a configuration error, not a skip.
"""
import compat_driver_smoke
from _topology import bolt_uri
import neo4j
from neo4j import GraphDatabase
from neo4j.exceptions import ClientError

def auth_smoke():
    """Preset-credential server (ADR-0126) behind the `--bolt-identity`
    shim (`--auth relate:ci-token --bolt-identity ...`): the port is
    a required topology fact like every other — unset is a configuration
    error, not a skip."""
    # right credentials work
    driver = GraphDatabase.driver(bolt_uri("RELATE_BOLT_PORT_AUTH_IDENTITY"),
                                   auth=("relate", "ci-token"))
    records, _, _ = driver.execute_query("RETURN 1 AS n")
    assert records[0]["n"] == 1
    driver.close()

    # wrong credentials are rejected
    driver = GraphDatabase.driver(bolt_uri("RELATE_BOLT_PORT_AUTH_IDENTITY"),
                                   auth=("relate", "wrong"))
    try:
        driver.execute_query("RETURN 1 AS n")
        raise AssertionError("wrong credentials must not connect")
    except ClientError as exc:
        assert "Unauthorized" in str(exc), f"expected Unauthorized, got {exc!r}"
    driver.close()
    print("auth smoke: preset accepted, wrong credentials rejected")


def path_smoke(driver):
    """Path hydration and nested graph types (packstream path encoder fix).

    Seeds one ``(:Person)-[:KNOWS {since: 2020}]->(:Person)`` edge and asserts
    the official driver hydrates ``neo4j.graph.Path`` / ``Node`` /
    ``Relationship`` instances even when nested inside lists and maps several
    levels deep (packstream 0x50/0x4E/0x52 as container elements)."""
    driver.execute_query(
        "MERGE (a:Person {name: $ada}) "
        "MERGE (b:Person {name: $bob}) "
        "MERGE (a)-[r:KNOWS]->(b) "
        "SET r.since = $since",
        ada="Ada",
        bob="Bob",
        since=2020,
    )

    records, _, _ = driver.execute_query("MATCH p=()-[]->() RETURN p")
    p = records[0]["p"]
    assert isinstance(p, neo4j.graph.Path), f"expected Path, got {type(p)!r}"
    assert len(p.relationships) == 1, (
        f"expected exactly one relationship, got {len(p.relationships)}"
    )
    rel = p.relationships[0]
    assert rel.type == "KNOWS", f"expected KNOWS, got {rel.type!r}"
    assert rel.get("since") == 2020, f"expected since=2020, got {rel.get('since')!r}"
    names = sorted(node.get("name") for node in p.nodes)
    assert names == ["Ada", "Bob"], f"expected Ada/Bob, got {names!r}"
    assert all("Person" in node.labels for node in p.nodes), (
        f"expected Person on both nodes, got {[node.labels for node in p.nodes]!r}"
    )

    records, _, _ = driver.execute_query(
        "MATCH p=(n)-[r]->() RETURN [p] AS l, {path: p, deep: [[p]]} AS m"
    )
    rec = records[0]
    assert isinstance(rec["l"][0], neo4j.graph.Path), (
        f"expected Path in list, got {type(rec['l'][0])!r}"
    )
    assert isinstance(rec["m"]["path"], neo4j.graph.Path), (
        f"expected Path in map, got {type(rec['m']['path'])!r}"
    )
    assert isinstance(rec["m"]["deep"][0][0], neo4j.graph.Path), (
        f"expected Path two levels deep, got {type(rec['m']['deep'][0][0])!r}"
    )

    records, _, _ = driver.execute_query(
        "MATCH p=(n)-[r]->() RETURN [n, r] AS pair, {n: n} AS m"
    )
    rec = records[0]
    assert isinstance(rec["pair"][0], neo4j.graph.Node), (
        f"expected Node in list, got {type(rec['pair'][0])!r}"
    )
    assert isinstance(rec["pair"][1], neo4j.graph.Relationship), (
        f"expected Relationship in list, got {type(rec['pair'][1])!r}"
    )
    assert isinstance(rec["m"]["n"], neo4j.graph.Node), (
        f"expected Node in map, got {type(rec['m']['n'])!r}"
    )
    print("path smoke: Path, Node and Relationship hydrate at depth")


def plan_smoke(driver):
    """EXPLAIN/PROFILE summary metadata through the official driver
    (ADR-0344/0345, ADR-0357): the plan rides the drained result's
    summary — Neo4j's placement — in the Neo4j dict shape, with the
    Neo4j-facing root args (the rendered ``string-representation`` and
    the planner/runtime identity) so tooling that reads them renders the
    plan."""
    with driver.session() as session:
        explained = session.run("EXPLAIN MATCH (n) RETURN n").consume()
        assert explained.plan is not None, "EXPLAIN summary carries the plan"
        assert explained.profile is None, "EXPLAIN carries no profile"
        root = explained.plan
        assert root["operatorType"] == "ProduceResults", root
        args = root["args"]
        assert "Cypher 5" in args["string-representation"], args
        assert "Planner relate" in args["string-representation"], args
        assert args["version"] == "5", args
        assert args["planner"] == "relate", args
        assert args["runtime"] == "relate", args
        assert args["planner-version"], args
        # The identifiers list is present in the Neo4j shape; this engine's
        # projection operators currently leave it empty (Neo4j names the
        # returned columns here) — the shape, not the content, is pinned
        # until the engine stamps projection identifiers.
        assert isinstance(root["identifiers"], list), root
        assert root["children"][0]["operatorType"] == "AllNodesScan", root

        profiled = session.run("PROFILE MATCH (n) RETURN n").consume()
        assert profiled.profile is not None, "PROFILE summary carries the profile"
        assert profiled.plan is None, "PROFILE carries no bare plan"
        profiled_args = profiled.profile["args"]
        for field in ("Rows", "TimeNs", "DbHits", "MemoryBytes"):
            assert field in profiled_args, (field, profiled_args)
        # The ROOT estimate mirrors Neo4j's float form (ADR-0357) — the
        # decoration converts the root's args; child nodes keep the
        # engine's integer form.
        if "EstimatedRows" in profiled_args:
            assert isinstance(profiled_args["EstimatedRows"], float), profiled_args

        plain = session.run("RETURN 1 AS n").consume()
        assert plain.plan is None and plain.profile is None, "plain runs carry neither"
    print("plan smoke: EXPLAIN/PROFILE summary metadata carries the plan shape")


def notification_smoke(driver):
    """EXPLAIN notifications through the official driver (ADR-0363
    decision 11): the CartesianProduct warning rides the ending summary's
    ``notifications`` in Neo4j's key shape, with the canonical remedy
    folded into ``description``."""
    # main() seeds only :Person; the two-pattern cartesian statement needs
    # a second, disjoint label.
    driver.execute_query("MERGE (:City {name: $name})", name="Oslo")
    _, summary, _ = driver.execute_query(
        "EXPLAIN MATCH (a:Person), (b:City) RETURN a, b"
    )
    notifications = summary.notifications
    codes = [notification["code"] for notification in notifications]
    cartesian = "Neo.ClientNotification.Statement.CartesianProduct"
    assert cartesian in codes, f"expected the cartesian notification, got {codes!r}"
    entry = notifications[codes.index(cartesian)]
    assert entry["title"] == "Cartesian product", entry
    assert entry["severity"] == "INFORMATION", entry
    assert entry["category"] == "PERFORMANCE", entry
    assert "(remedy: " in entry["description"], entry
    print("notification smoke: ending summary carries the cartesian notification")


def main():
    driver = GraphDatabase.driver(bolt_uri("RELATE_BOLT_PORT_IDENTITY"),
                                  auth=("neo4j", "unused"))
    records, _, _ = driver.execute_query("RETURN 1 AS n")
    assert records[0]["n"] == 1
    driver.execute_query("CREATE (p:Person {name: $name})", name="Ada")
    records, _, _ = driver.execute_query(
        "MATCH (p:Person) RETURN p.name AS name"
    )
    assert records[0]["name"] == "Ada"
    with driver.session() as session:
        with session.begin_transaction() as tx:
            tx.run("CREATE (n:Txn {name: $name})", name="Bob")
            name = tx.run("MATCH (n:Txn) RETURN n.name AS name").single()["name"]
            assert name == "Bob"
            tx.commit()
    records, _, _ = driver.execute_query("MATCH (n:Txn) RETURN n.name AS name")
    assert records[0]["name"] == "Bob"
    path_smoke(driver)
    plan_smoke(driver)
    notification_smoke(driver)
    # ADR-0117 §7 + ADR-0229 §1/§2: `database_` selects the namespace per
    # query — the one statement kind that still runs while routed to
    # `system_ontologies` is the administration set (ADR-0116 §2), and a
    # data query routed there fails with the 42N17 text.
    records, _, _ = driver.execute_query(
        "SHOW DATABASES", database_="system_ontologies"
    )
    assert len(records) == 3
    try:
        driver.execute_query(
            "MATCH (n) RETURN count(n) AS c", database_="system_ontologies"
        )
        raise AssertionError("a data query on system_ontologies must fail")
    except ClientError as e:
        assert (
            "This database is not queryable. Use the db.ontology.* procedures "
            "on the user database 'neo4j' instead." in str(e)
        ), e
    # An unknown database surfaces the CE text to the driver, with Neo4j's
    # DatabaseNotFound code (LABS-323 finding D-46).
    try:
        with driver.session(database="nosuch") as session:
            session.run("RETURN 1 AS n").consume()
        raise AssertionError("session(database='nosuch') must fail")
    except ClientError as e:
        assert "Graph not found: nosuch" in str(e), e
        assert e.code == "Neo.ClientError.Database.DatabaseNotFound", e
    # The LABS-323 compat fixes the official driver can observe, each
    # block tagged with its finding ids (tests/compat_driver_smoke.py).
    compat_driver_smoke.run(driver)
    driver.close()
    auth_smoke()
    print("ok")

if __name__ == "__main__":
    main()
