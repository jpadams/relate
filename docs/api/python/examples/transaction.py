"""An explicit transaction around a write.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/transaction.py
"""

import relate


def main():
    conn = relate.connect(":memory:")
    conn.autocommit = False
    conn.execute("CREATE (n:Person {name: 'Ada'})")
    rows = conn.execute("MATCH (n:Person) RETURN n.name AS name").fetchall()
    assert rows[0]["name"] == "Ada"
    conn.commit()
    conn.close()
    print("transaction example ok")


if __name__ == "__main__":
    main()
