import Foundation
import Relate

/// Serial, off-`MainActor` owner of the embedded Relate database.
///
/// `DatabaseService` is an `actor`: every query runs on the actor's executor, so
/// data loading and queries never block the `MainActor` while the UI renders. The
/// view model `await`s these methods and applies the results back on the
/// `MainActor` (see `MapViewModel`).
///
/// Every statement's log entry is yielded, in execution order, to `queryLog`:
/// the view model consumes that stream in one place, so no call site has to
/// remember to collect the entries its queries produced.
///
/// The actor owns one `Connection`. `Connection` is deliberately not
/// `Sendable` (the C ABI forbids sharing one across threads), so Swift 6
/// proves at compile time that only this actor ever touches it.
actor DatabaseService {
    private let conn: Connection
    private let logContinuation: AsyncStream<QueryLogEntry>.Continuation
    /// One entry per executed statement, successes and failures alike.
    nonisolated let queryLog: AsyncStream<QueryLogEntry>

    /// `":memory:"` for a throwaway store, or a directory for a durable one
    /// (WAL + checkpoints).
    init(path: String = ":memory:") throws {
        conn = try Database(path: path).connect()
        (queryLog, logContinuation) = AsyncStream.makeStream()
    }

    deinit {
        logContinuation.finish()
    }

    /// The app's durable store: `Application Support/SNOTEL.relate`.
    /// Application Support (not Documents) is the place for app-managed data
    /// the user never browses; `completeUntilFirstUserAuthentication` keeps
    /// the store writable while the device is locked after first unlock.
    static func persistentStoreURL() throws -> URL {
        let base = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true)
        let url = base.appendingPathComponent("SNOTEL.relate", isDirectory: true)
        #if os(iOS)
        try FileManager.default.createDirectory(
            at: url, withIntermediateDirectories: true,
            attributes: [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication])
        #else
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        #endif
        return url
    }

    /// Whether the store already holds the bundled dataset: its completion
    /// marker commits in the same transaction as the data, so a load that
    /// failed part-way never reads as loaded.
    func isLoaded() throws -> Bool {
        try count("MATCH (d:Dataset {name: 'snotel'}) RETURN count(d) AS count") > 0
    }

    /// Writes a checkpoint so the next launch opens from it instead of
    /// replaying the WAL.
    func checkpoint() throws {
        try conn.database.checkpoint()
    }

    // MARK: - Data Loading

    /// First-launch setup: reads and parses the bundled dataset on this actor
    /// (never the `MainActor`), loads it, and checkpoints so the next launch
    /// opens from a snapshot. A store that is already loaded is left as is.
    func loadBundledDataIfNeeded(bundle: Bundle? = nil) throws {
        guard try !isLoaded() else { return }
        try load(
            stations: DataLoader.loadStationsCSV(from: bundle),
            readings: DataLoader.loadReadingsCSV(from: bundle),
            basins: DataLoader.loadBasinsGeoJSON(from: bundle)
        )
        try checkpoint()
    }

    /// Loads one dataset all-or-nothing: every dataset's parameterized UNWIND
    /// (one statement per dataset, not one per row), the basin memberships and
    /// the completion marker `isLoaded()` probes commit in one transaction.
    /// The point index is schema, created idempotently ahead of it.
    func load(stations: [StationCSVRow], readings: [ReadingCSVRow], basins: [BasinFeature]) throws {
        try execute("CREATE POINT INDEX station_location IF NOT EXISTS FOR (s:Station) ON (s.location)")
        try conn.transaction { _ in
            try insertStations(stations)
            try insertReadings(readings)
            try insertBasins(basins)
            try insertBasinMemberships(stations: stations, basins: basins)
            try execute("CREATE (:Dataset {name: 'snotel'})")
        }
    }

    private func insertStations(_ stations: [StationCSVRow]) throws {
        let rows: [CypherValue] = stations.map { s in
            [
                "id": CypherValue(s.id),
                "name": CypherValue(s.name),
                "state": CypherValue(s.state),
                "latitude": CypherValue(s.latitude),
                "longitude": CypherValue(s.longitude),
                "elevation": CypherValue(s.elevationFt),
            ]
        }
        try execute(
            """
            UNWIND $rows AS row
            CREATE (:Station {
                id: row.id, name: row.name, state: row.state,
                latitude: row.latitude, longitude: row.longitude,
                elevation: row.elevation,
                location: point({latitude: row.latitude, longitude: row.longitude})
            })
            """,
            params: ["rows": .list(rows)]
        )
    }

    private func insertReadings(_ readings: [ReadingCSVRow]) throws {
        let rows: [CypherValue] = readings.map { r in
            [
                "stationId": CypherValue(r.stationId),
                "date": CypherValue(r.date),
                "swe": CypherValue(r.sweInches),
                "snowDepth": CypherValue(r.snowDepthInches),
                "temperature": CypherValue(r.tempAvgF),
            ]
        }
        try execute(
            """
            UNWIND $rows AS row
            MATCH (s:Station {id: row.stationId})
            CREATE (s)-[:HAS_READING]->(:Reading {
                date: row.date,
                swe_inches: row.swe,
                snow_depth_inches: row.snowDepth,
                temp_avg_f: row.temperature
            })
            """,
            params: ["rows": .list(rows)]
        )
    }

    private func insertBasins(_ basins: [BasinFeature]) throws {
        let rows: [CypherValue] = basins.map { b in
            [
                "name": CypherValue(b.name), "huc2": CypherValue(b.huc2),
                "geojson": CypherValue(b.geoJSON),
            ]
        }
        try execute(
            """
            UNWIND $rows AS row
            CREATE (:Basin {name: row.name, huc2: row.huc2, geojson: row.geojson})
            """,
            params: ["rows": .list(rows)]
        )
    }

    /// Materializes station-in-basin containment as `IN_BASIN` relationships,
    /// so basin queries are plain graph traversals.
    ///
    /// > Note: the containment test runs in Swift because the engine's
    /// > geospatial surface is the point — geometry tests happen in Swift
    /// > over the stored basin geometry. The native point functions
    /// > (`point.distance`, `point.withinBBox`) stay in Cypher below.
    private func insertBasinMemberships(stations: [StationCSVRow], basins: [BasinFeature]) throws {
        let memberships: [CypherValue] = stations.flatMap { s in
            basins
                .filter { $0.contains(latitude: s.latitude, longitude: s.longitude) }
                .map { b -> CypherValue in ["station": CypherValue(s.id), "basin": CypherValue(b.name)] }
        }
        try execute(
            """
            UNWIND $memberships AS m
            MATCH (s:Station {id: m.station}), (b:Basin {name: m.basin})
            CREATE (s)-[:IN_BASIN]->(b)
            """,
            params: ["memberships": .list(memberships)]
        )
    }

    // MARK: - Queries

    func allStationsWithSWE() throws -> [Station] {
        try stations(matching: "MATCH (s:Station)")
    }

    /// Every basin with its stored GeoJSON geometry and its snowpack
    /// aggregates: the mean SWE of its stations' readings and how many
    /// stations report one (nil and 0 for a basin without readings).
    func basins() throws -> [Basin] {
        try execute(
            """
            MATCH (b:Basin)
            OPTIONAL MATCH (b)<-[:IN_BASIN]-(s:Station)-[:HAS_READING]->(r:Reading)
            RETURN b.name AS name, b.huc2 AS huc2, b.geojson AS geojson,
                   avg(r.swe_inches) AS avgSWE, count(DISTINCT s) AS stationCount
            ORDER BY b.name
            """
        ).records.map(Basin.from)
    }

    func searchStations(_ searchText: String) throws -> [Station] {
        try stations(
            matching: """
            MATCH (s:Station)
            WHERE toLower(s.name) CONTAINS toLower($searchText)
               OR toLower(s.state) = toLower($searchText)
            """,
            params: ["searchText": CypherValue(searchText)]
        )
    }

    func stationsInBasin(basinName: String) throws -> [Station] {
        try stations(
            matching: "MATCH (s:Station)-[:IN_BASIN]->(:Basin {name: $basinName})",
            params: ["basinName": CypherValue(basinName)]
        )
    }

    func stationsNearby(lat: Double, lon: Double, radiusMeters: Double) throws -> [Station] {
        try stations(
            matching: """
            MATCH (s:Station)
            WHERE point.distance(
                s.location,
                point({latitude: $latitude, longitude: $longitude})
            ) < $radiusMeters
            """,
            extraColumns: ", point.distance(s.location, point({latitude: $latitude, longitude: $longitude})) AS distance",
            orderBy: "distance",
            params: [
                "latitude": CypherValue(lat), "longitude": CypherValue(lon),
                "radiusMeters": CypherValue(radiusMeters),
            ]
        )
    }

    func stationsInBBox(swLat: Double, swLon: Double, neLat: Double, neLon: Double) throws -> [Station] {
        try stations(
            matching: """
            MATCH (s:Station)
            WHERE point.withinBBox(
                s.location,
                point({latitude: $swLat, longitude: $swLon}),
                point({latitude: $neLat, longitude: $neLon})
            )
            """,
            params: [
                "swLat": CypherValue(swLat), "swLon": CypherValue(swLon),
                "neLat": CypherValue(neLat), "neLon": CypherValue(neLon),
            ]
        )
    }

    func databaseStats() throws -> DatabaseStats {
        DatabaseStats(
            stations: try count("MATCH (n:Station) RETURN count(n) AS count"),
            readings: try count("MATCH (n:Reading) RETURN count(n) AS count"),
            basins: try count("MATCH (n:Basin) RETURN count(n) AS count"),
            relationships: try count("MATCH ()-[r]->() RETURN count(r) AS count"),
            basinMemberships: try count("MATCH ()-[r:IN_BASIN]->() RETURN count(r) AS count")
        )
    }

    /// Runs a workbench statement; rendering the result is the view's job.
    func run(_ cypher: String) throws -> QueryResult {
        try execute(cypher)
    }

    /// The one station projection: `match` binds `s` (and filters it); every
    /// station query shares the OPTIONAL MATCH on readings, the columns
    /// `Station.from` decodes, and the ordering.
    private func stations(
        matching match: String,
        extraColumns: String = "",
        orderBy: String = "s.name",
        params: [String: CypherValue] = [:]
    ) throws -> [Station] {
        try execute(
            """
            \(match)
            OPTIONAL MATCH (s)-[:HAS_READING]->(r:Reading)
            RETURN s.id AS id, s.name AS name, s.state AS state,
                   s.latitude AS lat, s.longitude AS lon,
                   s.elevation AS elevation, r.swe_inches AS swe\(extraColumns)
            ORDER BY \(orderBy)
            """,
            params: params
        ).records.map(Station.from)
    }

    private func count(_ query: String) throws -> Int {
        guard let count = try execute(query).scalar().intValue else {
            throw RelateError.decoding("count() did not return an integer")
        }
        return Int(count)
    }

    @discardableResult
    private func execute(
        _ query: String,
        params: [String: CypherValue] = [:]
    ) throws -> QueryResult {
        let start = Date()
        do {
            let result = try conn.query(query, params)
            logContinuation.yield(
                QueryLogEntry(
                    cypher: query,
                    parameters: Self.parameterSummary(params),
                    // The engine's own parse + plan + execute time (ADR-0221).
                    durationMilliseconds: result.elapsedMs,
                    queryType: result.queryType,
                    rowCount: result.rows.count,
                    mutationCount: Self.mutationCount(result.stats),
                    outcome: .success
                )
            )
            return result
        } catch {
            logContinuation.yield(
                QueryLogEntry(
                    cypher: query,
                    parameters: Self.parameterSummary(params),
                    durationMilliseconds: Date().timeIntervalSince(start) * 1_000,
                    queryType: nil,
                    rowCount: 0,
                    mutationCount: 0,
                    outcome: .failure(error.localizedDescription)
                )
            )
            throw error
        }
    }

    private static func mutationCount(_ stats: QueryStats) -> Int {
        stats.nodesCreated + stats.nodesDeleted
            + stats.relationshipsCreated + stats.relationshipsDeleted
            + stats.propertiesSet + stats.labelsAdded
    }

    private static func parameterSummary(_ params: [String: CypherValue]) -> String? {
        guard !params.isEmpty else { return nil }
        return params.sorted { $0.key < $1.key }.map { key, param in
            let rawValue = param.description
            let value = rawValue.count > 100
                ? String(rawValue.prefix(97)) + "…"
                : rawValue
            return "$\(key) = \(value)"
        }.joined(separator: "\n")
    }
}
