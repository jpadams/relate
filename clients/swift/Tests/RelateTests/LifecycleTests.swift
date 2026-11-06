import Foundation
import Testing

@testable import Relate

@Suite struct LifecycleTests {
    @Test func transactionCommitsAndRollsBack() throws {
        let conn = try Database.inMemory().connect()
        try conn.transaction { try $0.query("CREATE (:Kept)") }
        struct Boom: Error {}
        #expect(throws: Boom.self) {
            try conn.transaction { tx in
                try tx.query("CREATE (:Dropped)")
                throw Boom()
            }
        }
        #expect(try conn.query("MATCH (n:Kept) RETURN count(n)").scalar() == 1)
        #expect(try conn.query("MATCH (n:Dropped) RETURN count(n)").scalar() == 0)
    }

    @Test func persistsAcrossReopenAndReadOnly() throws {
        try withTemporaryStore { path in
            do {
                let db = try Database(path: path)
                try db.connect().query("CREATE (:P {v: $v})", ["v": 7])
                try db.checkpoint()
                try db.connect().query("CREATE (:P {v: 8})")  // WAL-only tail
            }
            do {
                let db = try Database(readOnlyPath: path)
                #expect(db.isReadOnly)
                let conn = try db.connect()
                #expect(try conn.query("MATCH (p:P) RETURN sum(p.v)").scalar() == 15)
                #expect(throws: RelateError.self) { try conn.query("CREATE (:P)") }
            }
        }
    }

    @Test func aSecondWritableOpenOfOneStoreIsRefused() throws {
        try withTemporaryStore { path in
            let first = try Database(path: path)
            #expect(throws: RelateError.self) { try Database(path: path) }
            _ = first
        }
    }

    @Test func connectionsOnSeparateTasksShareOneDatabase() async throws {
        let db = try Database.inMemory()
        try db.connect().query("CREATE (:Seed)")
        let counts = try await withThrowingTaskGroup(of: Int64.self) { group in
            for _ in 0..<8 {
                group.addTask {
                    let conn = try db.connect()
                    return try #require(conn.query("MATCH (n) RETURN count(n)").scalar().intValue)
                }
            }
            return try await group.reduce(into: []) { $0.append($1) }
        }
        #expect(counts == Array(repeating: 1, count: 8))
    }
}
