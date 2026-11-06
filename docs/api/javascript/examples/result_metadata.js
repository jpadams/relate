const { Database } = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();

// Every query result carries the statement's resolved namespace and
// plan-time query type (ADR-0285) alongside columns and rows.
const explained = conn.query("EXPLAIN CREATE (n)");
// EXPLAIN reports the wrapped statement's query type without executing
// it: the CREATE never runs, so the graph stays empty.
if (explained.queryType !== "w") throw new Error("unexpected");
if (explained.namespace !== "neo4j") throw new Error("unexpected");
if (conn.query("MATCH (n) RETURN n").rows.length !== 0) throw new Error("unexpected");

// Query type value set (ADR-0162, ADR-0332): "r" read, "w" a write with no
// returned columns, "rw" a write with returned columns, "s" schema.
if (conn.query("MATCH (n) RETURN n").queryType !== "r") throw new Error("unexpected");
if (conn.query("CREATE (n)").queryType !== "w") throw new Error("unexpected");

// A USE-routed statement reports its resolved namespace; the session
// namespace is unchanged, so a following unrouted statement still
// resolves to "neo4j".
const routed = conn.query("USE system SHOW DATABASES YIELD name RETURN name");
if (routed.namespace !== "system") throw new Error("unexpected");
if (conn.query("RETURN 1 AS n").namespace !== "neo4j") throw new Error("unexpected");

// consume().elapsedMs is the engine-reported wall-clock milliseconds for
// the statement (ADR-0221) — parse, plan and run, not client marshalling.
// Its phase split (ADR-0491) — parseMs / planMs / executeMs — says which
// phase the time went to; the three sub-windows never sum past the total.
const timed = conn.query("RETURN 1 AS n").consume();
if (!Number.isFinite(timed.elapsedMs) || timed.elapsedMs < 0) throw new Error("unexpected");
if (timed.parseMs + timed.planMs + timed.executeMs > timed.elapsedMs) throw new Error("unexpected");
if (timed.parseMs < 0 || timed.planMs < 0 || timed.executeMs < 0) throw new Error("unexpected");

// Per-value kinds (ADR-0502 §1): every cell carries its own kind through
// the typed value walker, so a column can mix shapes freely.
// `typeof` on the decoded cell is the JS-side discrimination.
if (typeof conn.query("RETURN 1 AS n").single().n !== "number") throw new Error("unexpected");
if (conn.query("RETURN 1 AS n, 'x' AS s").single().s !== "x") throw new Error("unexpected");

// Query notifications (ADR-0363): the engine-native list rides every result,
// always present. A disconnected multi-pattern EXPLAIN plans as a Cartesian
// product on a store holding both labels; a plain run carries the empty list.
conn.query("CREATE (:Person)");
conn.query("CREATE (:City)");
const planned = conn.query("EXPLAIN MATCH (a:Person), (b:City) RETURN a, b");
if (planned.notifications.length !== 1) throw new Error("unexpected");
if (planned.notifications[0].code !== "Neo.ClientNotification.Statement.CartesianProduct") {
  throw new Error("unexpected");
}
if (planned.notifications[0].severity !== "INFORMATION") throw new Error("unexpected");
if (planned.consume().notifications.length !== 1) throw new Error("unexpected");
if (conn.query("RETURN 1 AS n").notifications.length !== 0) throw new Error("unexpected");
