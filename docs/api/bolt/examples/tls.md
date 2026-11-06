# Example: Bolt over TLS with a self-signed certificate

> **Retired start command (ADR-0368).** Bolt TLS was reached through the
> standalone `relate-bolt` binary (`--tls-cert`/`--tls-key`, optional
> `--tls-client-ca`), which is retired (LABS-381). The capability is
> **library-level only** now — no product binary exposes it. For a product
> deployment, front `relate serve` with a TLS-terminating proxy (the
> container's `--entrypoint /bin/relate-bolt` TLS alternative is gone
> too); to exercise TLS directly, embed `relate_bolt::serve` with the
> certificate pair. The protocol and behavior below are unchanged.

Generate a self-signed certificate for the run (its subjectAltName must
match the host the driver dials, because it is self-signed):

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout server.key -out server.pem -subj "/CN=127.0.0.1" \
  -addext "subjectAltName=IP:127.0.0.1"
# After the run: rm -f server.key server.pem
```

With the official Neo4j Python driver installed
(`python3 -m pip install neo4j`), the [driver example](tls.py) runs against
a TLS listener started through the library entry point (`serve(&Args)`
with `--tls-cert server.pem --tls-key server.key`, bound to `127.0.0.1` on
the example's default port 7688); it is run by the crate's own test
harness in-process (ADR-0368 §3), not as a documented product-binary
command pair:

```bash
python3 docs/api/bolt/examples/tls.py
```

It connects with `bolt+ssc://`, which accepts the self-signed certificate,
asserts `RETURN 1 AS n` returns `1`, and prints `ok`.

For a CA-trusted server certificate, dial `bolt+s://` instead. To require
client certificates, give the listener `--tls-client-ca client-ca.pem`
and the driver the matching client key pair. Certificates and keys reload
only when the server process restarts.
