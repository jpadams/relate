# SNOTEL Explorer (iOS)

A SwiftUI + MapKit app that embeds Relate in-process to explore snowpack
(snow water equivalent, SWE) at ~50 western-US SNOTEL stations, grouped into
river basins. No server, no network: the graph lives in the app's
Application Support directory as a Relate store (WAL + Parquet
checkpoint).

It is the example consumer of the Swift surface (ADR-0433,
[`docs/api/swift`](../../docs/api/swift/spec.md)); CI builds it in the `swift`
job.

## What it shows

- **Durable embedded store** — first launch loads the bundled CSV/GeoJSON and
  checkpoints; later launches reopen the checkpoint and skip the load.
- **Batched, all-or-nothing load** — one parameterized `UNWIND $rows` per
  dataset, all inside a single `conn.transaction { … }` together with the
  completion marker the next launch checks, so an interrupted load never
  reads as loaded.
- **Graph traversal** — `(:Station)-[:HAS_READING]->(:Reading)` and
  `(:Station)-[:IN_BASIN]->(:Basin)`.
- **Native point functions** — `point.distance` (nearby radius, distance
  column) and `point.withinBBox`, backed by a `POINT INDEX`.
- **Result metadata** — the Query Log shows each statement's engine-reported
  time (`elapsedMs`), query type (READ / WRITE / SCHEMA) and mutation counters.
- **Swift 6 concurrency** — a `DatabaseService` actor owns the (non-`Sendable`)
  `Connection`; the `@MainActor` view model awaits it.

> **Note** Basin membership (`IN_BASIN`) is computed in Swift at load time
> with a point-in-polygon test, because the Cypher surface is points only:
> `point.distance` and `point.withinBBox` are native, and basin geometry
> travels as a stored GeoJSON property for the map overlay.

## Requirements

- Xcode 16+ (Swift 6 language mode), iOS 17+ simulator or device
- The Rust toolchain pinned by `rust-toolchain.toml`, plus the Apple targets:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios \
  aarch64-apple-darwin x86_64-apple-darwin
```

## Build and run

The app builds against the engine in this checkout, not a released binary,
so build the XCFramework first and set `RELATE_LOCAL_XCFRAMEWORK=1`
(the root `Package.swift` reads it):

```sh
# From the repository root: everything CI runs, including this app
make test-swift

# Or step by step
scripts/build-xcframework.sh
export RELATE_LOCAL_XCFRAMEWORK=1
cd examples/ios-snotel && swift test          # core library tests on the Mac
xcodebuild -project SNOTELExplorer.xcodeproj -scheme SNOTELExplorer \
  -destination 'generic/platform=iOS Simulator' build
```

In Xcode: quit it first (`open --env` only reaches a newly launched
process), then `open -a Xcode --env RELATE_LOCAL_XCFRAMEWORK=1
SNOTELExplorer.xcodeproj` so package resolution sees the variable, and press
**Cmd+R**. Until the first release stamps a binary into `Package.swift`, the
local XCFramework is used either way. The project depends on the repository-root `Relate` package
(`../..`).

## Layout

```
SNOTELExplorer/
  Data/          bundled stations + readings CSV, basins GeoJSON
  Models/        Station, Basin, SWELevel, DemoQuery, QueryLogEntry
  Services/      DataLoader (CSV/GeoJSON, point-in-polygon), DatabaseService (actor)
  ViewModels/    MapViewModel (@MainActor, @Observable)
  Views/         map, filter panel, insights, query workbench, query log
Tests/           Swift Testing suites for the SNOTELCore library target
Package.swift    SNOTELCore library + tests (everything except Views and the App)
```

## Using Relate from Swift

```swift
import Relate

let db = try Database(path: storeURL.path)      // or Database.inMemory()
let conn = try db.connect()
let result = try conn.query(
    "MATCH (s:Station)-[:IN_BASIN]->(:Basin {name: $basin}) RETURN s.name AS name",
    ["basin": "Pacific Northwest"])
for record in result.records { print(record["name"]?.stringValue ?? "") }
print(result.queryType, result.elapsedMs, result.stats.nodesCreated)
```

Data: the station list and April 1 readings are a bundled sample derived
from USDA NRCS SNOTEL; basin polygons are simplified HUC2-region outlines for
demonstration, not survey-grade boundaries.
