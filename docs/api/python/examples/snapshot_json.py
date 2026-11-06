"""Open an empty directory with RELATE_SNAPSHOT_FORMAT=json (ADR-0050)."""

import os
import tempfile

import relate


def main():
    os.environ["RELATE_SNAPSHOT_FORMAT"] = "json"
    with tempfile.TemporaryDirectory() as tmp:
        conn = relate.connect(tmp)
        conn.execute("CREATE (n:Person {name: 'Ada'})")


if __name__ == "__main__":
    main()
