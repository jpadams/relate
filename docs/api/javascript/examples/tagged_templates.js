// Tagged-template queries (ADR-0222): interpolated values become auto-named
// $p0, $p1, … parameters — injection-safe by construction. Run from a
// checkout with the FFI library built (or RELATE_LIB set):
// node docs/api/javascript/examples/tagged_templates.js

const { Database } = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();

// Interpolated values are parameters, never statement text.
const name = "Alice";
conn.query`CREATE (p:Person {name: ${name}, age: ${36}})`;

// Values that contain Cypher-looking text stay data.
const hostile = "x'} DETACH DELETE p //";
conn.query`CREATE (p:Person {name: ${hostile}})`;

// The same primitive value twice maps to one parameter (objects by
// identity); nested arrays and objects are values too.
const hobbies = ["climbing", "go"];
conn.query`CREATE (p:Person {name: ${"Carol"}, hobbies: ${hobbies}})`;

const rows = conn.query`
  MATCH (p:Person)
  WHERE p.age >= ${30} OR p.name = ${hostile}
  RETURN p.name AS name
  ORDER BY name
`.rows;
console.log(rows.map((r) => r.name).join(", ")); // Alice, x'}... (Carol has no age)

db.checkpoint();
