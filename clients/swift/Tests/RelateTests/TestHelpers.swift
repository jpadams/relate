import Foundation

/// One policy for a throwaway on-disk store: a UUID-named directory under
/// the system temp, removed when the body returns.
func withTemporaryStore(_ body: (String) throws -> Void) throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("relate-swift-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: dir) }
    try body(dir.path)
}

