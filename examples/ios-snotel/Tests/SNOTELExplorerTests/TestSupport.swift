import Foundation

/// A bundle rooted at a freshly-created empty directory, so every resource lookup fails.
func makeEmptyBundle() throws -> Bundle {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("SNOTELEmptyBundle-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    guard let bundle = Bundle(url: dir) else {
        struct EmptyBundleUnavailable: Error {}
        throw EmptyBundleUnavailable()
    }
    return bundle
}
