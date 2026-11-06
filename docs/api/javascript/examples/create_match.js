const { Database } = require("@relate/relate");

const db = Database.open(":memory:");
const conn = db.connect();
conn.query("CREATE (n:Person {name: 'Ada'})");
const result = conn.query("MATCH (n:Person) RETURN n.name AS name");
if (result.rows[0].name !== "Ada") {
  throw new Error("unexpected");
}
