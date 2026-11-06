# Example: streamable HTTP with curl

The HTTP transport is the `serve --mcp PORT` shape: a network listener
that takes one complete JSON-RPC message per `POST` and answers
`application/json`. Every request carries the RFC 7617 Basic credential
(ADR-0232 §1).

Start the server with a fixed credential:

```bash
cargo run -p relate-cli -- serve \
  --database /tmp/graph.db --mcp 8000 --auth neo4j:s3cret
```

Drive it with `curl`:

```bash
# Unauthenticated: 401 with the challenge.
curl -i -X POST localhost:8000 \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
# HTTP/1.1 401 Unauthorized
# WWW-Authenticate: Basic realm="relate"

# Authenticated roundtrip.
curl -u neo4j:s3cret -X POST localhost:8000 \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'

curl -u neo4j:s3cret -X POST localhost:8000 \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write_cypher","arguments":{"statement":"CREATE (:Person {name: \"Ada\"})"}}}'

curl -u neo4j:s3cret -X POST localhost:8000 \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_schema","arguments":{}}}'
```

The transport is stateless per request: there is no session header to
track, so the three calls above may hit the server in any order after
`initialize`. The 401/200 roundtrip and the header shape are what
`tests/http_transport.rs` checks by driving the handler directly.
