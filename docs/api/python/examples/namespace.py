"""Session namespace selection (ADR-0117 §4).

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/namespace.py
"""

import relate


def main():
    # The namespace defaults to "neo4j"; names match case-insensitively.
    conn = relate.connect(":memory:", namespace="NEO4J")
    assert conn.namespace == "neo4j"

    # Switch namespaces between transactions.
    conn.set_namespace("system")
    assert conn.namespace == "system"
    conn.set_namespace("neo4j")
    assert conn.namespace == "neo4j"

    # An unknown namespace fails with DatabaseNotFound -> OperationalError.
    try:
        relate.connect(":memory:", namespace="nosuch")
    except relate.OperationalError as e:
        assert e.args[0].endswith("Graph not found: nosuch")
    else:
        raise AssertionError("expected OperationalError for an unknown namespace")

    # A namespace cannot change while a transaction is open.
    conn.autocommit = False
    conn.execute("CREATE (:Person {name: 'Ada'})")
    try:
        conn.set_namespace("system")
    except relate.OperationalError as e:
        assert "Cannot change the namespace while a transaction is open." in e.args[0]
    else:
        raise AssertionError("expected OperationalError inside a transaction")
    finally:
        conn.rollback()

    conn.close()
    print("namespace example OK")


if __name__ == "__main__":
    main()
