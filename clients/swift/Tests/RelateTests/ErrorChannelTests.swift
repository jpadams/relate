import Foundation
import Testing

@testable import Relate

@Suite struct ErrorChannelTests {
    @Test func aStatementFailureRidesTheResult() throws {
        let conn = try Database.inMemory().connect()
        let error = try #require(throws: RelateError.self) { try conn.query("RETURN 1/0") }
        let engine = try #require(error.engineError)
        #expect(engine.code == "Neo.ClientError.Statement.ArithmeticError")
        #expect(engine.gqlStatus == "22G03")
        #expect(engine.stage == .runtime)
        #expect(!engine.retryable)
        #expect(!engine.message.isEmpty)
    }

    @Test func aSyntaxErrorIsACompileStageFailure() throws {
        let conn = try Database.inMemory().connect()
        let error = try #require(throws: RelateError.self) { try conn.query("RETURN nope(") }
        let engine = try #require(error.engineError)
        #expect(engine.stage == .compile && !engine.retryable)
        #expect(engine.code.hasPrefix("Neo.ClientError."))
    }

    @Test func callFailuresTravelWithTheCall() throws {
        try withTemporaryStore { path in
            let first = try Database(path: path)
            try first.connect().query("CREATE (:P)")
            // A second writable open fails through the open call's own
            // out-param — the same typed shape, not a thread-local slot.
            let second = try #require(throws: RelateError.self) { try Database(path: path) }
            #expect(second.engineError != nil)
            let reader = try Database(readOnlyPath: path)
            let write = try #require(throws: RelateError.self) {
                try reader.connect().query("CREATE (:P)")
            }
            #expect(write.engineError?.retryable == false)
        }
    }
}
