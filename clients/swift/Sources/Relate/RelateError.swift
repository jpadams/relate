import Foundation

/// The typed error shape every C ABI v2 failure carries (ADR-0502 §2) —
/// classified by the engine's own single table, so the client never
/// classifies an error itself. Branch on `code` / `stage`, retry on
/// `retryable`, never on `message` text. A pure model: the channel's JSON
/// decode lives at the boundary (CABI.swift's `engineError`).
public struct EngineError: Sendable, Hashable {
    /// The Neo4j status code (`Neo.<Classification>.<Category>.<Title>`),
    /// passed through unchanged so a `code`-based classifier reaches the
    /// same verdict.
    public let code: String
    /// The GQLSTATUS (five characters).
    public let gqlStatus: String
    /// Where the failure happened: `compile` (parse/plan), `runtime`
    /// (execution) or `internal` — a failure the engine never saw: a NULL
    /// or poisoned handle, a caught panic, an argument that failed to
    /// decode, a params-builder refusal.
    public let stage: Stage
    /// The drivers' transient test: `true` exactly when re-running the same
    /// work may succeed (today the write-write conflict and the retryable
    /// workspace cap).
    public let retryable: Bool
    public let message: String

    public enum Stage: String, Sendable, Hashable {
        case compile, runtime, `internal`
    }

    init(code: String, gqlStatus: String, stage: Stage, retryable: Bool, message: String) {
        self.code = code
        self.gqlStatus = gqlStatus
        self.stage = stage
        self.retryable = retryable
        self.message = message
    }
}

/// Every error the Swift surface throws.
public enum RelateError: LocalizedError, Sendable, Equatable, CustomStringConvertible {
    /// The engine (or the C boundary) refused the call; the typed shape
    /// travels with the call that failed — there is no thread-local slot.
    case engine(EngineError)
    /// A parameter the wrapper cannot encode for the C ABI.
    case invalidParameter(String)
    /// The C ABI returned a payload that violates its documented contract.
    case decoding(String)

    public var message: String {
        switch self {
        case .engine(let e): e.message
        case .invalidParameter(let m), .decoding(let m): m
        }
    }

    /// The typed shape when this is an `.engine` error, `nil` otherwise.
    public var engineError: EngineError? {
        if case .engine(let e) = self { e } else { nil }
    }

    public var description: String {
        switch self {
        case .engine(let e): "Relate: [\(e.code)] \(e.message)"
        case .invalidParameter(let m): "Relate invalid parameter: \(m)"
        case .decoding(let m): "Relate decoding error: \(m)"
        }
    }

    /// What `error.localizedDescription` shows (otherwise Foundation prints
    /// "The operation couldn't be completed").
    public var errorDescription: String? { description }
}
