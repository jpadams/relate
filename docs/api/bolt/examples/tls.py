"""Run the documented Bolt TLS example against a self-signed-certificate server.

Requires a TLS listener started with --tls-cert/--tls-key — library-level
only since ADR-0368; no product binary exposes it (see tls.md).
Run from the repository root:

    python3 docs/api/bolt/examples/tls.py [HOST] [PORT]

HOST defaults to 127.0.0.1 and PORT to 7688; both may also be set with
RELATE_BOLT_HOST / RELATE_BOLT_PORT.
"""

import os
import sys

from neo4j import GraphDatabase


host = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("RELATE_BOLT_HOST", "127.0.0.1")
port = sys.argv[2] if len(sys.argv) > 2 else os.environ.get("RELATE_BOLT_PORT", "7688")

# bolt+ssc:// accepts the server's self-signed certificate.
with GraphDatabase.driver(
    f"bolt+ssc://{host}:{port}", auth=("neo4j", "unused")
) as driver:
    records, _, _ = driver.execute_query("RETURN 1 AS n")
    assert records[0]["n"] == 1, records

print("ok")
