import Relate
import Foundation

struct QueryLogEntry: Identifiable, Equatable, Sendable {
    enum Outcome: Equatable, Sendable {
        case success
        case failure(String)
    }

    let id: UUID = UUID()
    let timestamp: Date = Date()
    let cypher: String
    let parameters: String?
    let durationMilliseconds: Double
    /// The engine's plan-time classification; `nil` when the statement failed.
    let queryType: QueryType?
    let rowCount: Int
    let mutationCount: Int
    let outcome: Outcome

    var operation: String {
        cypher
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .split(whereSeparator: \.isWhitespace)
            .first?
            .uppercased() ?? "QUERY"
    }

    /// Short badge text for the engine query type.
    var queryTypeLabel: String? {
        switch queryType {
        case .read: "READ"
        case .write: "WRITE"
        case .readWrite: "READ/WRITE"
        case .schema: "SCHEMA"
        case nil: nil
        }
    }

    var succeeded: Bool { failureMessage == nil }

    var failureMessage: String? {
        if case .failure(let message) = outcome { message } else { nil }
    }
}
