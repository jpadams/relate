// Result ergonomics (ADR-0216): rows by column name, single(), consume(),
// and graph(). Run from a checkout with the FFI library built (or
// RELATE_LIB set): node docs/api/javascript/examples/result_ergonomics.js

const { Database } = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();

// Rows are objects keyed by column name; stats carry the 11 counters.
conn.query("CREATE (a:Person {name: $name})-[:KNOWS]->(b:Person {name: 'Bob'})", {
  name: "Alice",
});
const created = conn.query("CREATE (c:Person {name: 'Carol'})");
if (created.stats.nodes_created !== 1) throw new Error("stats missing");

// single(): the one row, or ResultNotSingleError.
const one = conn.query("MATCH (p:Person {name: 'Alice'}) RETURN p.name AS name").single();
if (one.name !== "Alice") throw new Error("single() failed");

// consume(): counters, resolved namespace, query type, engine elapsed and
// its phase split (ADR-0491).
const summary = conn.query("MATCH (p:Person) RETURN p.name AS name").consume();
console.log(
  `${summary.queryType} in ${summary.elapsedMs.toFixed(2)} ms ` +
    `(parse ${summary.parseMs.toFixed(2)} / plan ${summary.planMs.toFixed(2)} / ` +
    `execute ${summary.executeMs.toFixed(2)}), namespace ${summary.namespace}`
);

// graph(): the result's unique entities, de-duplicated by id.
const graph = conn
  .query("MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a, r, b")
  .graph();
console.log(`graph: ${graph.nodes.length} nodes, ${graph.relationships.length} relationships`);

db.checkpoint();
