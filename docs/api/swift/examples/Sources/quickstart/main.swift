import Relate

let db = try Database.inMemory()
let conn = try db.connect()

try conn.query(
    "CREATE (:Person {name: $name, born: $born})-[:KNOWS]->(:Person {name: 'Bob'})",
    ["name": "Ada", "born": 1815])

let result = try conn.query(
    "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS from, b.name AS to, a.born AS born")
for record in result.records {
    print(record["from"]!, "knows", record["to"]!)
}

precondition(result.columns == ["from", "to", "born"])
precondition(result.rows == [["Ada", "Bob", 1815]])
precondition(result.records[0]["from"]?.stringValue == "Ada")
