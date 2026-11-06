"""Run the documented Bolt LOAD CSV example against an opted-in server.

Requires people.csv (Ada and Lin) and too_large.csv (8 MiB + 1 byte) in
the server's --load-csv-dir (library-level only since ADR-0368; no product
binary exposes it — see load_csv.md).
"""

from neo4j import GraphDatabase
from neo4j.exceptions import Neo4jError


with GraphDatabase.driver("bolt://127.0.0.1:7687", auth=("neo4j", "unused")) as driver:
    records, _, _ = driver.execute_query(
        "LOAD CSV FROM 'people.csv' AS row RETURN row[0] AS name"
    )
    assert [record["name"] for record in records] == ["Ada", "Lin"]

    try:
        driver.execute_query("LOAD CSV FROM 'too_large.csv' AS row RETURN row")
    except Neo4jError as error:
        assert "LOAD CSV file exceeds byte limit" in str(error), error
        assert "too_large.csv" not in str(error), error
    else:
        raise AssertionError("expected LOAD CSV byte-limit rejection")
