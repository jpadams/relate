#!/usr/bin/env python3
"""Regenerate (or validate) the pinned Neo4j Community Edition error fixture.

The fixture at testdata/ce-errors/neo4j-ce.json pins the exact error texts
Neo4j Community Edition produces for the database-administration and
graph-routing cases that ADR-0116/ADR-0117 model in Relate. Engine tests
assert against this file, so the CE text is authoritative: if a live capture
disagrees with the ADR wording, record the live text and add the case to the
divergence list printed by this script.

Field semantics
---------------

gqlstatus        pinned ROOT-CAUSE GQLSTATUS for the case. Values come from
                 the ROOT_CAUSE_CODES table below, maintained from the
                 neo4j/neo4j 2026.09 CE acceptance-test and GQLSTATUS sources
                 (the same sources the original "acceptance-tests" fixture
                 cited); each entry keeps its source_url citation. The live
                 capture cross-checks the server's first GQLSTATUS cause
                 (the code directly below the top-level status) against the
                 table and warns on mismatch (the driver does not expose a
                 public gql_status_object; the chain is reachable via the
                 private `_gql_cause` attribute).
gqlstatus_wire   the top-level GQLSTATUS the live driver sees on the wire
                 (42001 for most syntax/semantic errors, 51N27/42N18 where
                 top-level and root cause coincide).
harness          optional runner variant (see HARNESS below): how the fixture
                 runner executes the case when a plain statement run on
                 `database` does not exercise the CE behavior.
neo4j_code       the legacy Neo4j status code (Neo.ClientError.*) over Bolt.
message          the live CE message with the position decoration stripped
                 (see POSITION_DECORATION).

Usage
-----

Validate the committed fixture without a server:

    python3 scripts/ce_error_fixtures.py --fixtures-only

Regenerate from live Neo4j Community servers (requires the `neo4j` Python
driver and Docker). Two containers are needed: a plain read-write one and a
read-only one. A fresh store CANNOT boot with server.databases.read_only set
(first-boot token/index creation writes), so the read-only store is
initialized in two passes on a named volume: boot a plain container on the
volume first, remove it, then start the read-only container on the same
volume.

    docker volume create neo4j-ce-ro-data

    # main read-write container
    docker run -d --name neo4j-ce-errors -p 7687:7687 -p 7474:7474 \
        -e NEO4J_AUTH=neo4j/password123 neo4j:2026.09-community

    # read-only pass 1: initialize the store read-write on the named volume
    docker run -d --name neo4j-ce-errors-ro-init \
        -e NEO4J_AUTH=neo4j/password123 \
        -v neo4j-ce-ro-data:/data neo4j:2026.09-community
    docker logs -f neo4j-ce-errors-ro-init   # wait for "Started."
    docker rm -f neo4j-ce-errors-ro-init

    # read-only pass 2: boot the initialized store with neo4j read-only
    docker run -d --name neo4j-ce-errors-ro -p 7688:7687 \
        -e NEO4J_AUTH=neo4j/password123 \
        -e NEO4J_server_databases_read__only=neo4j \
        -v neo4j-ce-ro-data:/data neo4j:2026.09-community

    python3 scripts/ce_error_fixtures.py \
        --uri bolt://localhost:7687 --read-only-uri bolt://localhost:7688 \
        --user neo4j --password password123 --write

    docker rm -f neo4j-ce-errors neo4j-ce-errors-ro

The script waits for the target database to report currentStatus=online
before running cases (a write attempted while the read-only database is
still starting surfaces as 08N09 DatabaseUnavailable, not the 42N18 pin).

Without --write the script captures everything and prints a diff summary
against the committed fixture without touching it.

When no Docker daemon is reachable the fixture is maintained by hand from
neo4j/neo4j branch 2026.09 CE acceptance tests and GQLSTATUS sources
(source: "acceptance-tests"); every case then carries a source_url.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
import time
from pathlib import Path

# CE appends " (line L, column C (offset: O))\n\"<statement>\"\n <caret>" to
# some error messages; the pin is the core text before it.
POSITION_DECORATION = re.compile(r' \(line \d+, column \d+ \(offset: \d+\)\)\n"[\s\S]*$')

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURE_PATH = REPO_ROOT / "testdata" / "ce-errors" / "neo4j-ce.json"

REQUIRED_TOP_LEVEL = ("neo4j_version", "source", "captured_at", "cases", "show_databases")
REQUIRED_CASE_FIELDS = (
    "id",
    "statement",
    "database",
    "outcome",
    "gqlstatus",
    "gqlstatus_wire",
    "neo4j_code",
    "message",
)
SHOW_DATABASES_FIELDS = (
    "name",
    "type",
    "access",
    "currentStatus",
    "requestedStatus",
    "default",
    "home",
    "aliases",
    "constituents",
)

# Statements sent to the default (rw) server. Each entry maps to the fixture
# case of the same id.
DEFAULT_SERVER_CASES = {
    "create_database": ("neo4j", "CREATE DATABASE foo"),
    "drop_database": ("neo4j", "DROP DATABASE foo"),
    "start_database": ("neo4j", "START DATABASE foo"),
    "stop_database": ("neo4j", "STOP DATABASE foo"),
    "create_alias": ("neo4j", "CREATE ALIAS a FOR DATABASE foo"),
    "alter_database_access": ("neo4j", "ALTER DATABASE neo4j SET ACCESS READ ONLY"),
    "cypher_prefix_create_database": ("neo4j", "CYPHER 25 CREATE DATABASE foo"),
    "alter_unknown_database": ("system", "ALTER DATABASE nosuch SET DEFAULT LANGUAGE CYPHER 5"),
    "alter_unknown_database_if_exists": (
        "system",
        "ALTER DATABASE nosuch IF EXISTS SET DEFAULT LANGUAGE CYPHER 5",
    ),
    "use_unknown_graph": ("neo4j", "USE nosuch RETURN 1"),
    "use_graph_byname": ("neo4j", "USE graph.byName('neo4j') RETURN 1"),
    "graph_names": ("neo4j", "RETURN graph.names()"),
    "multi_graph_union": ("neo4j", "USE neo4j RETURN 1 AS x UNION USE system RETURN 2 AS x"),
    "multi_graph_call": ("neo4j", "USE neo4j CALL { USE system RETURN 2 AS x } RETURN x"),
    "byelementid_unknown": (
        "neo4j",
        "USE graph.byElementId('4:00000000-0000-0000-0000-000000000000:n1') RETURN 1",
    ),
    "system_data_clause": ("neo4j", "USE system MATCH (n) RETURN count(n)"),
    "use_system_show_databases_with": (
        "system",
        "USE system SHOW DATABASES YIELD name AS d1 WITH 1 AS n RETURN n",
    ),
    "use_neo4j_show_databases": ("neo4j", "USE neo4j SHOW DATABASES"),
    "database_does_not_exist": ("nosuch", "RETURN 1"),
}

READ_ONLY_CASES = {
    "read_only_write": ("neo4j", "CREATE ()"),
    "read_only_call_subquery_return": ("neo4j", "CALL { RETURN 1 AS x }"),
    "read_only_call_subquery_match": ("neo4j", "CALL { MATCH (n) RETURN n }"),
    "read_only_call_subquery_create": ("neo4j", "CALL { CREATE (n) }"),
}

# Runner harness variants: cases the fixture runner must NOT execute as a
# plain statement on `database` — the harness is part of the case's behavior,
# so it is modeled here and emitted into the fixture, not re-derived by the
# runner from case ids. `connect_namespace`: the Rust equivalent of CE's
# session-scoped open is `connect_namespace(database)` itself failing.
# `read_only_namespace`: run the statement on the read-only namespace
# (`system_ontologies`, ADR-0116 §4) instead of `database`.
HARNESS = {
    "database_does_not_exist": "connect_namespace",
    "read_only_write": "read_only_namespace",
    "read_only_call_subquery_return": "read_only_namespace",
    "read_only_call_subquery_match": "read_only_namespace",
    "read_only_call_subquery_create": "read_only_namespace",
}

_COMMUNITY_IT = (
    "https://github.com/neo4j/neo4j/blob/2026.09/community/community-it/cypher-it"
    "/src/test/scala/org/neo4j/cypher"
)

# Pinned root-cause GQLSTATUS per error case (success cases pin 00000 and are
# not listed). Values are maintained from the neo4j/neo4j 2026.09 CE
# acceptance-test and GQLSTATUS sources cited per entry; the live capture
# cross-checks them against the server's GQLSTATUS cause chain.
ROOT_CAUSE_CODES = {
    "create_database": {
        "gqlstatus": "51N27",
        "status_description": "error: system configuration or operation exception - not supported in this edition. 'CREATE DATABASE' is not supported in community edition.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "drop_database": {
        "gqlstatus": "51N27",
        "status_description": "error: system configuration or operation exception - not supported in this edition. 'DROP DATABASE' is not supported in community edition.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "start_database": {
        "gqlstatus": "51N27",
        "status_description": "error: system configuration or operation exception - not supported in this edition. 'START DATABASE' is not supported in community edition.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "stop_database": {
        "gqlstatus": "51N27",
        "status_description": "error: system configuration or operation exception - not supported in this edition. 'STOP DATABASE' is not supported in community edition.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "create_alias": {
        "gqlstatus": "51N27",
        "note": "CE rejects CREATE ALIAS; the acceptance test asserts the message only, the 51N27 GQLSTATUS is the common UnsupportedAdministrationCommand status.",
        "source_url": f"{_COMMUNITY_IT}/CommunityAliasAdministrationCommandAcceptanceTest.scala",
    },
    "alter_database_access": {
        "gqlstatus": "51N27",
        "status_description": "error: system configuration or operation exception - not supported in this edition. 'ALTER DATABASE SET ACCESS' is not supported in community edition.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "cypher_prefix_create_database": {
        "gqlstatus": "51N27",
        "note": "The statement is echoed without the leading `CYPHER 25 ` prefix (evidenced by the prefixed ALTER DATABASE cases in the same test class).",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "alter_unknown_database": {
        "gqlstatus": "42N00",
        "status_description": "error: syntax error or access rule violation - graph reference not found. A graph reference with the name `nosuch` was not found. Verify that the spelling is correct.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/graphdb-api/src/main/java/org/neo4j/dbms/api/DatabaseNotFoundHelper.java",
    },
    "use_unknown_graph": {
        "gqlstatus": "42N00",
        "status_description": "error: syntax error or access rule violation - graph reference not found. A graph reference with the name `nosuch` was not found. Verify that the spelling is correct.",
        "source_url": f"{_COMMUNITY_IT}/CommunityQueryRoutingAcceptanceTest.scala",
    },
    "use_graph_byname": {
        "gqlstatus": "42N72",
        "status_description": "error: syntax error or access rule violation - graph function only supported on composite databases. Calling graph functions is only supported on composite databases. Use the name directly or connect to a composite database with the desired constituents.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/front-end/ast/src/main/scala/org/neo4j/cypher/internal/ast/Clause.scala",
    },
    "graph_names": {
        "gqlstatus": "42N48",
        "note": "graph.names and graph.propertiesByName are marked enterpriseOnly in CE's builtInFunctions.json and are not registered on CE, so the call fails as an unknown function, not with 42N72.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/community-it/cypher-it-test-util/src/main/resources/builtInFunctions.json",
    },
    "multi_graph_union": {
        "gqlstatus": "42NA5",
        "status_description": "error: syntax error or access rule violation - accessing multiple graphs only supported on composite databases. Accessing multiple graphs in the same query is only supported on composite databases. Connect to a composite database with the desired constituents.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/front-end/frontend/src/main/scala/org/neo4j/cypher/messages/MessageUtilProvider.scala",
    },
    "multi_graph_call": {
        "gqlstatus": "42NA5",
        "status_description": "error: syntax error or access rule violation - accessing multiple graphs only supported on composite databases. Accessing multiple graphs in the same query is only supported on composite databases. Connect to a composite database with the desired constituents.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/community-it/cypher-it/src/test/scala/org/neo4j/cypher/CommunityQueryRoutingAcceptanceTest.scala",
    },
    "byelementid_unknown": {
        "gqlstatus": "42NA7",
        "status_description": "error: syntax error or access rule violation - referenced database not found. No database is corresponding to `graph.byElementId(4:00000000-0000-0000-0000-000000000000:n1)`. Verify that the elementId is correct.",
        "note": "The middle segment must be a well-formed UUID; a non-UUID segment such as 'doesnotexist' fails earlier in ElementIdDecoder with an IllegalArgumentException before reaching this error.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/graphdb-api/src/main/java/org/neo4j/dbms/api/DatabaseNotFoundHelper.java",
    },
    "system_data_clause": {
        "gqlstatus": "42N17",
        "status_description": "error: syntax error or access rule violation - unsupported request. 'MATCH' is not allowed on the system database.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/cypher-planner/src/main/scala/org/neo4j/cypher/internal/compiler/AdministrationCommandPlanBuilder.scala",
    },
    "use_system_show_databases_with": {
        "gqlstatus": "42N17",
        "status_description": "error: syntax error or access rule violation - unsupported request. 'WITH' is not allowed on the system database.",
        "note": "Live CE 2026.09 chains 42001 -> 42N17 -> 42NA9; the wire message is 'The following clauses are not allowed on a system database: WITH.' — the §4 whitelist's composability completion (ADR-0116 §4, 2026-11-04).",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/cypher-planner/src/main/scala/org/neo4j/cypher/internal/compiler/AdministrationCommandPlanBuilder.scala",
    },
    "use_neo4j_show_databases": {
        "gqlstatus": "42N79",
        "status_description": "error: syntax error or access rule violation - invalid USE clause. The USE clause is not required for administration commands. Retry your query without the USE clause, and it will be routed automatically.",
        "note": "CE's auto-routing rejection for any USE clause on a database-administration command (bare or composed, UNION arm included); Relate keeps `USE system` + administration command accepted as a recorded divergence.",
        "source_url": f"{_COMMUNITY_IT}/CommunityMultiDatabaseAdministrationCommandAcceptanceTest.scala",
    },
    "read_only_write": {
        "gqlstatus": "42N18",
        "status_description": "error: syntax error or access rule violation - read-only database. The database is in read-only mode.",
        "note": "Requires the server to be started with server.databases.read_only=neo4j (Docker: -e NEO4J_server_databases_read__only=neo4j) on a pre-initialized store; see the module docstring for the two-pass setup.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/graphdb-api/src/main/java/org/neo4j/graphdb/WriteOperationsNotAllowedException.java",
    },
    "read_only_call_subquery_return": {
        "gqlstatus": "42N71",
        "note": "Captured 2026-09-30 on the read-only server (server.databases.read_only=neo4j): CE rejects a read-only top-level CALL { } at semantic-check time with 42N71 (a query may not conclude with CALL), before read-only enforcement runs. Relate classifies a top-level CALL { } as a write outright and fails 42N18 instead — the outcome matches (CE does not allow read-only top-level CALL { } on a read-only database either), only the code and layer differ. Relate's behavior is pinned by system_ontologies_rejects_top_level_call_subqueries_with_42n18 in relate/relate/tests/namespace.rs.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/front-end/ast/src/main/scala/org/neo4j/cypher/internal/ast/semantics/SemanticError.scala",
    },
    "read_only_call_subquery_match": {
        "gqlstatus": "42N71",
        "note": "Captured 2026-09-30 on the read-only server (server.databases.read_only=neo4j): CE rejects a read-only top-level CALL { } at semantic-check time with 42N71 (a query may not conclude with CALL), before read-only enforcement runs. Relate classifies a top-level CALL { } as a write outright and fails 42N18 instead — the outcome matches (CE does not allow read-only top-level CALL { } on a read-only database either), only the code and layer differ. Relate's behavior is pinned by system_ontologies_rejects_top_level_call_subqueries_with_42n18 in relate/relate/tests/namespace.rs.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/cypher/front-end/ast/src/main/scala/org/neo4j/cypher/internal/ast/semantics/SemanticError.scala",
    },
    "read_only_call_subquery_create": {
        "gqlstatus": "42N18",
        "status_description": "error: syntax error or access rule violation - read-only database. The database is in read-only mode.",
        "note": "Captured 2026-09-30 on the read-only server (server.databases.read_only=neo4j): the writing control case reaches execution and fails 42N18, exactly the error Relate's outright write classification of a top-level CALL { } produces on system_ontologies.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/graphdb-api/src/main/java/org/neo4j/graphdb/WriteOperationsNotAllowedException.java",
    },
    "database_does_not_exist": {
        "gqlstatus": "42N00",
        "status_description": "error: syntax error or access rule violation - graph reference not found. A graph reference with the name `nosuch` was not found. Verify that the spelling is correct.",
        "note": "Session-scoped: the driver opens a session with database='nosuch' and the error surfaces on the first RUN. Live 2026.09 CE wires 42001 with a 42N00 cause and the text 'Graph not found: nosuch'; ADR-0146 adopted the live text and codes.",
        "source_url": "https://github.com/neo4j/neo4j/blob/2026.09/community/bolt/src/main/java/org/neo4j/bolt/tx/error/NoSuchDatabaseTransactionCreationException.java",
    },
}

# Captured live texts that differ from the wording quoted in ADR-0116/0117.
# CE text is authoritative; divergences are noted, not edited away. Kept in
# sync with the `note` fields in the fixture.
DIVERGENCES = [
    {
        "case": "read_only_write",
        "adr": "ADR-0116: 'No write operations are allowed on this database. The database is in read-only mode.'",
        "ce": "No write operations are allowed on this database. The database is in read-only mode on this Neo4j instance.",
    },
    {
        "case": "multi_graph_union",
        "adr": "ADR-0116: 'Multiple graphs in the same query not allowed here. This feature is only available on composite databases.'",
        "ce": "CE appends a second line: '\\nAttempted to access graph system'.",
    },
    {
        "case": "multi_graph_call",
        "adr": "ADR-0116: 'Multiple graphs in the same query not allowed here. This feature is only available on composite databases.'",
        "ce": "CE appends a second line: '\\nAttempted to access graph system'.",
    },
    {
        "case": "graph_names",
        "adr": "ADR-0116: graph.names fails with 42N72 and stays registered/listed by SHOW FUNCTIONS.",
        "ce": "On CE, graph.names is enterpriseOnly (not listed by SHOW FUNCTIONS) and fails with \"Unknown function 'graph.names'\" (42N48).",
    },
    {
        "case": "database_does_not_exist",
        "adr": "ADR-0117 §6 quoted 'Database does not exist. Database name: 'nosuch'.' with root-cause GQLSTATUS 22N51 (top-level 22000); ADR-0146 supersedes it and adopts the live text and codes.",
        "ce": "Live 2026.09 CE fails with 'Graph not found: nosuch' (Neo.ClientError.Database.DatabaseNotFound), wire GQLSTATUS 42001 with a 42N00 cause.",
    },
]


def validate(fixture: dict) -> list[str]:
    """Return a list of structural problems with the fixture (empty if ok)."""
    problems: list[str] = []
    for key in REQUIRED_TOP_LEVEL:
        if key not in fixture:
            problems.append(f"missing top-level key: {key}")
    if problems:
        return problems

    if not isinstance(fixture["cases"], list) or not fixture["cases"]:
        problems.append("cases must be a non-empty list")
    if not isinstance(fixture["show_databases"], list) or not fixture["show_databases"]:
        problems.append("show_databases must be a non-empty list")

    seen = set()
    for case in fixture.get("cases", []):
        cid = case.get("id", "<no id>")
        for field in REQUIRED_CASE_FIELDS:
            value = case.get(field)
            if not isinstance(value, str) or not value:
                problems.append(f"case {cid}: field {field} must be a non-empty string")
        expected = ROOT_CAUSE_CODES.get(cid)
        if expected is not None and case.get("gqlstatus") != expected["gqlstatus"]:
            problems.append(
                f"case {cid}: gqlstatus {case.get('gqlstatus')} != pinned root cause {expected['gqlstatus']}"
            )
        seen.add(cid)

    expected_ids = set(DEFAULT_SERVER_CASES) | set(READ_ONLY_CASES)
    for cid in sorted(expected_ids - seen):
        problems.append(f"missing case: {cid}")

    names = set()
    for row in fixture.get("show_databases", []):
        for field in SHOW_DATABASES_FIELDS:
            if field not in row:
                problems.append(f"show_databases row {row.get('name', '?')}: missing {field}")
        names.add(row.get("name"))
    for name in ("neo4j", "system"):
        if name not in names:
            problems.append(f"show_databases: missing row for {name}")
    return problems


def wait_for_database_online(driver, name: str, timeout: float = 120.0) -> None:
    """Block until `name` reports currentStatus=online (or raise SystemExit)."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with driver.session(database="system") as session:
                rows = session.run(
                    "SHOW DATABASES YIELD name, currentStatus RETURN name, currentStatus"
                ).data()
            status = next((r["currentStatus"] for r in rows if r["name"] == name), None)
            if status == "online":
                return
        except Exception:
            pass  # server not up yet
        time.sleep(2)
    raise SystemExit(f"database {name} did not come online within {timeout:.0f}s")


def live_cause_status(exc: BaseException) -> str | None:
    """First GQLSTATUS cause below the top-level status, if hydrated.

    The pinned "root cause" convention (inherited from the acceptance-tests
    fixture) is the first cause on the chain, not the deepest: e.g.
    system_data_clause chains 42001 -> 42N17 -> 42NA9 and pins 42N17. The
    neo4j Python driver does not expose a public gql_status_object; the chain
    is reachable via the private `_gql_cause` attribute (driver 6.x).
    """
    cause = getattr(exc, "_gql_cause", None)
    if cause is None:
        return None
    return getattr(cause, "gql_status", None)


def capture_live(uri: str, read_only_uri: str | None, user: str, password: str) -> dict:
    """Run every case against a live CE server and build a fixture dict."""
    from neo4j import GraphDatabase  # imported lazily; only needed for live capture

    mismatches: list[str] = []

    def run_case(driver, cid: str, database: str, statement: str) -> dict:
        pinned = ROOT_CAUSE_CODES.get(cid)
        try:
            with driver.session(database=database) as session:
                session.run(statement).consume()
        except Exception as exc:  # neo4j.exceptions.Neo4jException and friends
            wire = getattr(exc, "gql_status", None) or ""
            cause_status = live_cause_status(exc)
            if pinned is not None and cause_status is not None and cause_status != pinned["gqlstatus"]:
                mismatches.append(
                    f"{cid}: live cause {cause_status} != pinned {pinned['gqlstatus']}"
                )
            # CE appends position decoration (" (line 1, column 8 (offset: 7))\n\"<statement>\"\n ^")
            # to some messages. The pin is the core text; strip the decoration.
            message = getattr(exc, "message", str(exc))
            message = POSITION_DECORATION.sub("", message)
            case = {
                "outcome": "error",
                "gqlstatus": pinned["gqlstatus"] if pinned else (cause_status or wire),
                "gqlstatus_wire": wire,
                "neo4j_code": getattr(exc, "code", "") or "",
                "message": message,
            }
            if pinned:
                for key in ("status_description", "note", "source_url"):
                    if key in pinned:
                        case[key] = pinned[key]
            return case
        return {
            "outcome": "success",
            "gqlstatus": "00000",
            "gqlstatus_wire": "00000",
            "neo4j_code": "none (statement succeeds)",
            "message": "CE accepts this command",
        }

    def collect_cases(driver, case_map: dict) -> list[dict]:
        collected = []
        for cid, (database, statement) in case_map.items():
            result = run_case(driver, cid, database, statement)
            harness = {"harness": HARNESS[cid]} if cid in HARNESS else {}
            collected.append(
                {"id": cid, "statement": statement, "database": database, **harness, **result}
            )
        return collected

    cases = []
    with GraphDatabase.driver(uri, auth=(user, password)) as driver:
        server_info = driver.get_server_info()
        version = getattr(server_info, "agent", "unknown")
        wait_for_database_online(driver, "neo4j")
        cases.extend(collect_cases(driver, DEFAULT_SERVER_CASES))

        with driver.session(database="system") as session:
            rows = session.run(
                "SHOW DATABASES YIELD name, type, access, currentStatus, requestedStatus, "
                "default, home, aliases, constituents RETURN *"
            ).data()
        show_databases = [row for row in rows if row["name"] in ("neo4j", "system")]

    if read_only_uri:
        with GraphDatabase.driver(read_only_uri, auth=(user, password)) as ro_driver:
            # A write attempted before the read-only database is online
            # surfaces as 08N09 DatabaseUnavailable, not the 42N18 pin.
            wait_for_database_online(ro_driver, "neo4j")
            cases.extend(collect_cases(ro_driver, READ_ONLY_CASES))

    from datetime import datetime, timezone

    for mismatch in mismatches:
        print(f"WARNING: {mismatch}", file=sys.stderr)

    return {
        "neo4j_version": version,
        "source": "live-container",
        "captured_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "notes": [
            "gqlstatus is the pinned root-cause GQLSTATUS from ROOT_CAUSE_CODES in scripts/ce_error_fixtures.py (maintained from neo4j/neo4j 2026.09 CE acceptance-test/GQLSTATUS sources; per-case source_url). gqlstatus_wire is the top-level GQLSTATUS the live driver sees.",
            "message is the live CE text with the position decoration stripped; CE canonicalizes string literals to double quotes in the attempted-graph echo.",
            "Where the live text differs from the wording quoted in ADR-0116/ADR-0117 the case is listed in the script's DIVERGENCES; CE text is authoritative.",
        ],
        "cases": cases,
        "show_databases": show_databases,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fixtures-only", action="store_true",
                        help="validate the committed fixture; no server needed")
    parser.add_argument("--uri", default="bolt://localhost:7687", help="read-write CE Bolt URI")
    parser.add_argument("--read-only-uri", default=None,
                        help="Bolt URI of the CE container started with NEO4J_server_databases_read__only=neo4j")
    parser.add_argument("--user", default="neo4j")
    parser.add_argument("--password", default="password123")
    parser.add_argument("--write", action="store_true", help="overwrite the fixture with live capture")
    args = parser.parse_args()

    if args.fixtures_only:
        fixture = json.loads(FIXTURE_PATH.read_text())
        problems = validate(fixture)
        if problems:
            for problem in problems:
                print(f"ERROR: {problem}", file=sys.stderr)
            return 1
        case_ids = [case["id"] for case in fixture["cases"]]
        print(f"fixture ok: {len(case_ids)} cases: {', '.join(case_ids)}")
        print(f"source: {fixture['source']} (neo4j {fixture['neo4j_version']})")
        return 0

    captured = capture_live(args.uri, args.read_only_uri, args.user, args.password)
    problems = validate(captured)
    for problem in problems:
        print(f"WARNING: captured fixture: {problem}", file=sys.stderr)

    new_text = json.dumps(captured, indent=2, sort_keys=False) + "\n"
    if args.write:
        FIXTURE_PATH.write_text(new_text)
        print(f"wrote {FIXTURE_PATH}")
    else:
        old_text = FIXTURE_PATH.read_text() if FIXTURE_PATH.exists() else ""
        diff = difflib.unified_diff(old_text.splitlines(), new_text.splitlines(), lineterm="")
        print("\n".join(diff))
    if DIVERGENCES:
        print("known divergences from ADR-0116/0117 wording (CE text is authoritative):")
        print(json.dumps(DIVERGENCES, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
