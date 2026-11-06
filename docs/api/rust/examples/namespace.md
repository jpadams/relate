# Namespace example

Run with `cargo run -p relate --example namespace`.

Every store has three fixed namespaces (ADR-0116 §1): `neo4j` (the user data
graph), `system` (SHOW commands, supported administration commands and
procedure calls only) and `system_ontologies` (listed but not queryable,
ADR-0229). The example connects with the default `neo4j` session namespace,
writes user data, selects a namespace with `USE`, opens a `system` session
with `Database::connect_namespace("SYSTEM")` (names match
case-insensitively), runs `SHOW DATABASES` there, connects with a typed
`Namespace` (`Database::connect_typed` — no name parsing, ADR-0150),
switches to `system_ontologies` — where only the ADR-0116 §2
administration commands still run and every other statement fails with the
42N17 text — and switches a connection's session namespace with
`Connection::set_namespace` between transactions. Each `QueryResult`
reports the namespace its statement ran in (ADR-0117 §7).
