// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SNOTELExplorer",
    platforms: [.iOS(.v17), .macOS(.v14)],
    dependencies: [
        // The repository-root Relate package (build its binary target
        // first: scripts/build-xcframework.sh).
        .package(path: "../.."),
    ],
    targets: [
        // Core library (models, services, data loading, view models) — testable, no UI-framework deps
        .target(
            name: "SNOTELCore",
            dependencies: [
                .product(name: "Relate", package: "relate"),
            ],
            path: "SNOTELExplorer",
            exclude: ["Views", "SNOTELExplorerApp.swift"],
            resources: [
                .process("Data"),
            ]
        ),
        // Tests (core library only, no simulator or UI frameworks needed)
        .testTarget(
            name: "SNOTELExplorerTests",
            dependencies: ["SNOTELCore"],
            path: "Tests/SNOTELExplorerTests"
        ),
    ],
    swiftLanguageModes: [.v6]
)
