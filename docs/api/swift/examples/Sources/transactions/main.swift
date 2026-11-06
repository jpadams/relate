import Relate

let conn = try Database.inMemory().connect()

// transaction(_:) commits when the body returns ...
try conn.transaction { tx in
    try tx.query("CREATE (:Account {id: 1, balance: 100})")
    try tx.query("CREATE (:Account {id: 2, balance: 0})")
}

// ... and rolls back, then rethrows, when it throws.
struct InsufficientFunds: Error {}
do {
    try conn.transaction { tx in
        try tx.query("MATCH (a:Account {id: 1}) SET a.balance = a.balance - 500")
        let balance = try tx.query("MATCH (a:Account {id: 1}) RETURN a.balance").scalar()
        if (balance.intValue ?? 0) < 0 { throw InsufficientFunds() }
    }
    preconditionFailure("the transfer should have been refused")
} catch is InsufficientFunds {
    print("transfer refused; rolled back")
}

// Explicit begin/commit/rollback are available too.
try conn.begin()
try conn.query("CREATE (:Temp)")
try conn.rollback()

let balance = try conn.query("MATCH (a:Account {id: 1}) RETURN a.balance").scalar()
let temps = try conn.query("MATCH (t:Temp) RETURN count(t)").scalar()
print("balance:", balance)
precondition(balance == 100)
precondition(temps == 0)
