import Testing
import Foundation
import Relate
@testable import SNOTELCore

@Suite("DatabaseService Tests")
struct DatabaseServiceTests {

    private func makeService() async throws -> DatabaseService {
        let svc = try DatabaseService()
        try await svc.load(
            stations: DataLoader.parseStationsCSV(sampleStationsCSV),
            readings: DataLoader.parseReadingsCSV(sampleReadingsCSV),
            basins: DataLoader.parseBasinsGeoJSON(sampleBasinsJSON)
        )
        return svc
    }

    @Test("A durable store reopens loaded and skips the bundled load")
    func durableStoreReopens() async throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("snotel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        do {
            let svc = try DatabaseService(path: dir.path)
            #expect(try await !svc.isLoaded())
            try await svc.loadBundledDataIfNeeded()
            #expect(try await svc.isLoaded())
        }
        let reopened = try DatabaseService(path: dir.path)
        #expect(try await reopened.isLoaded())
        try await reopened.loadBundledDataIfNeeded()
        #expect(try await reopened.allStationsWithSWE().count == 50)
    }

    @Test("A failed load commits nothing and does not read as loaded")
    func failedLoadIsAtomic() async throws {
        let svc = try DatabaseService()
        // NaN is a valid FLOAT parameter under the typed ABI, so the trigger
        // is a coordinate point() refuses: an out-of-range latitude appended
        // after the valid stations fails the stations statement once earlier
        // rows were created, inside the load transaction.
        let badStation = StationCSVRow(id: 999_999, name: "Nowhere", state: "XX",
                                       latitude: 200, longitude: 0, elevationFt: 0)
        let stations = try DataLoader.parseStationsCSV(sampleStationsCSV) + [badStation]
        await #expect(throws: (any Error).self) {
            try await svc.load(stations: stations, readings: [], basins: [])
        }
        #expect(try await !svc.isLoaded())
        #expect(try await svc.allStationsWithSWE().isEmpty)
    }

    @Test("Load stations and query all")
    func loadAndQueryStations() async throws {
        let svc = try await makeService()
        let stations = try await svc.allStationsWithSWE()
        #expect(stations.count == 3)
        #expect(stations.contains { $0.name == "Bogus Basin" })
    }

    @Test("Query stations in basin via IN_BASIN")
    func stationsInBasin() async throws {
        let svc = try await makeService()
        let pnw = try await svc.stationsInBasin(basinName: "Pacific Northwest")
        // Blewett Pass (WA) and Bumping Ridge (WA) are in PNW; Bogus Basin (ID) is not
        let names = pnw.map(\.name)
        #expect(names.contains("Blewett Pass"))
        #expect(names.contains("Bumping Ridge"))
        #expect(!names.contains("Bogus Basin"))
    }

    @Test("Nearby stations using point.distance")
    func nearbyStations() async throws {
        let svc = try await makeService()
        // Search near Blewett Pass (47.53, -120.62), 200km radius
        let nearby = try await svc.stationsNearby(lat: 47.53, lon: -120.62, radiusMeters: 200_000)
        #expect(nearby.count >= 1)
        #expect(nearby.first?.name == "Blewett Pass") // closest
        // Verify distance is populated
        #expect(nearby.first?.distanceMeters != nil)
    }

    @Test("Bounding box query")
    func boundingBoxQuery() async throws {
        let svc = try await makeService()
        // Box around Washington state
        let inBox = try await svc.stationsInBBox(swLat: 46.0, swLon: -122.0, neLat: 49.0, neLon: -119.0)
        let names = inBox.map(\.name)
        #expect(names.contains("Blewett Pass"))
        #expect(names.contains("Bumping Ridge"))
        #expect(!names.contains("Bogus Basin"))
    }

    @Test("Basins carry geometry and snowpack aggregates")
    func basinAggregates() async throws {
        let svc = try await makeService()
        let basins = try await svc.basins()
        #expect(basins.count == 2)
        let pnw = try #require(basins.first { $0.name == "Pacific Northwest" })
        #expect(pnw.avgSWE != nil)
        #expect(pnw.stationCount == 2)
        #expect(pnw.geoJSON.contains("Polygon"))
    }

    @Test("Query parameters preserve station names with punctuation")
    func parameterizedStationValues() async throws {
        let svc = try DatabaseService()
        try await svc.load(stations: [
            StationCSVRow(
                id: 100,
                name: "O'Brien's \\ Snow Course",
                state: "CO",
                latitude: 40,
                longitude: -105,
                elevationFt: 9_000
            ),
        ], readings: [], basins: [])
        let stations = try await svc.allStationsWithSWE()
        #expect(stations.first?.name == "O'Brien's \\ Snow Course")
    }

    @Test("Spatial containment can be materialized as graph relationships")
    func basinMembershipRelationships() async throws {
        let svc = try await makeService()
        let stats = try await svc.databaseStats()
        #expect(stats.stations == 3)
        #expect(stats.readings == 3)
        #expect(stats.basins == 2)
        #expect(stats.nodes == 8)
        #expect(stats.basinMemberships == 3)
        #expect(stats.relationships == 6)
    }

    @Test("Station search matches names and state abbreviations")
    func stationSearch() async throws {
        let svc = try await makeService()
        #expect(try await svc.searchStations("Blewett").map(\.name) == ["Blewett Pass"])
        #expect(try await svc.searchStations("WA").count == 2)
    }

    @Test("Curated demo queries execute and return tabular output", arguments: DemoQuery.examples)
    func demoQueries(query: DemoQuery) async throws {
        let svc = try await makeService()
        let result = try await svc.run(query.cypher)
        #expect(!result.columns.isEmpty)
        #expect(result.rows.allSatisfy { $0.count == result.columns.count })
        #expect(result.elapsedMs >= 0)
    }

    @Test("Query logger records successful and failed executions")
    func queryLogging() async throws {
        let svc = try DatabaseService()
        var log = svc.queryLog.makeAsyncIterator()
        try await svc.load(stations: [
            StationCSVRow(
                id: 1,
                name: "Logger Station",
                state: "CO",
                latitude: 40,
                longitude: -105,
                elevationFt: 8_000
            ),
        ], readings: [], basins: [])
        // The load's entries: the index, the four UNWINDs, the marker.
        let loadEntries = await next(6, from: &log)
        #expect(loadEntries.count == 6)
        #expect(loadEntries[1].operation == "UNWIND")
        #expect(loadEntries[1].queryType == .write)
        #expect(loadEntries[1].parameters?.contains("Logger Station") == true)
        #expect(loadEntries[1].mutationCount > 0)

        _ = try await svc.allStationsWithSWE()

        do {
            _ = try await svc.run("THIS IS NOT CYPHER")
            Issue.record("Invalid Cypher unexpectedly succeeded")
        } catch {
            // The failure is still logged.
        }

        let entries = await next(2, from: &log)
        #expect(entries.count == 2)
        #expect(entries[0].queryType == .read)
        #expect(entries[0].rowCount == 1)
        #expect(entries[1].succeeded == false)
    }
}

/// The next `count` query-log entries, in execution order.
private func next(
    _ count: Int, from log: inout AsyncStream<QueryLogEntry>.Iterator
) async -> [QueryLogEntry] {
    var entries: [QueryLogEntry] = []
    while entries.count < count, let entry = await log.next() {
        entries.append(entry)
    }
    return entries
}

// MARK: - Sample Test Data

private let sampleStationsCSV = """
id,name,state,latitude,longitude,elevation_ft
515,Bogus Basin,ID,43.7667,-116.1,6040
526,Blewett Pass,WA,47.5333,-120.6167,4240
978,Bumping Ridge,WA,46.8833,-121.3,4600
"""

private let sampleReadingsCSV = """
station_id,date,swe_inches,snow_depth_inches,temp_avg_f
515,2024-04-01,8.1,22.0,38
526,2024-04-01,14.2,38.0,35
978,2024-04-01,18.7,48.0,33
"""

private let sampleBasinsJSON: Data = """
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "properties": { "name": "Pacific Northwest", "huc2": "17" },
      "geometry": {
        "type": "Polygon",
        "coordinates": [[
          [-125.0, 42.0], [-125.0, 49.0], [-117.0, 49.0],
          [-117.0, 46.5], [-116.0, 46.0], [-116.5, 44.5],
          [-117.5, 44.0], [-117.5, 42.0], [-125.0, 42.0]
        ]]
      }
    },
    {
      "type": "Feature",
      "properties": { "name": "Upper Snake", "huc2": "17-04" },
      "geometry": {
        "type": "Polygon",
        "coordinates": [[
          [-117.5, 42.0], [-116.5, 44.5], [-114.5, 44.5],
          [-111.0, 44.5], [-111.0, 42.0], [-114.5, 42.0],
          [-117.5, 42.0]
        ]]
      }
    }
  ]
}
""".data(using: .utf8)!
