"""Create nodes and match them back.

Run from the repo root after `pip install ./surfaces/relate-python`:

    python docs/api/python/examples/create_match.py
"""

import relate


def main():
    db = relate.Database(":memory:")
    db.execute("CREATE (n:Person {name: 'Ada'})")
    result = db.execute("MATCH (n:Person) RETURN n.name AS name")
    assert result.columns == ["name"]
    assert result.data[0]["name"] == "Ada"
    db.close()
    print("create_match example ok")


if __name__ == "__main__":
    main()
