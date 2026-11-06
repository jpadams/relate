import Testing
import Relate
@testable import SNOTELCore

@Suite("Model Tests")
struct ModelTests {

    @Test("SWELevel classification")
    func sweLevel() {
        #expect(SWELevel.from(swe: nil) == .noData)
        #expect(SWELevel.from(swe: 0) == .low)
        #expect(SWELevel.from(swe: 5) == .low)
        #expect(SWELevel.from(swe: 9.9) == .low)
        #expect(SWELevel.from(swe: 10) == .moderate)
        #expect(SWELevel.from(swe: 15) == .moderate)
        #expect(SWELevel.from(swe: 20) == .high)
        #expect(SWELevel.from(swe: 34.9) == .high)
        #expect(SWELevel.from(swe: 35) == .veryHigh)
        #expect(SWELevel.from(swe: 50) == .veryHigh)
    }

    @Test("Station.from parses CypherValue row")
    func stationFromRow() throws {
        let row: [String: CypherValue] = [
            "id": .int(302),
            "name": .string("Aneroid Lake"),
            "state": .string("OR"),
            "lat": .float(45.0833),
            "lon": .float(-117.15),
            "elevation": .int(7500),
            "swe": .float(28.5),
        ]
        let station = try Station.from(row: row)
        #expect(station.id == 302)
        #expect(station.name == "Aneroid Lake")
        #expect(station.elevation == 7500)
        #expect(station.sweInches == 28.5)
        #expect(station.sweLevel == .high)
    }

    @Test("Station.from handles missing SWE")
    func stationMissingSWE() throws {
        let row: [String: CypherValue] = [
            "id": .int(1),
            "name": .string("Test"),
            "state": .string("XX"),
            "lat": .float(40.0),
            "lon": .float(-110.0),
            "elevation": .int(5000),
        ]
        let station = try Station.from(row: row)
        #expect(station.sweInches == nil)
        #expect(station.sweLevel == .noData)
    }

    @Test("Station.from refuses a row missing a guaranteed column")
    func stationMissingElevation() {
        let row: [String: CypherValue] = [
            "id": .int(1),
            "name": .string("Test"),
            "state": .string("XX"),
            "lat": .float(40.0),
            "lon": .float(-110.0),
        ]
        #expect(throws: RelateError.self) { try Station.from(row: row) }
    }

    @Test("Basin.from parses CypherValue row")
    func basinFromRow() throws {
        let row: [String: CypherValue] = [
            "name": .string("Pacific Northwest"),
            "huc2": .string("17"),
            "geojson": .string("{\"type\":\"Polygon\"}"),
            "avgSWE": .float(22.5),
            "stationCount": .int(12),
        ]
        let basin = try Basin.from(row: row)
        #expect(basin.name == "Pacific Northwest")
        #expect(basin.avgSWE == 22.5)
        #expect(basin.stationCount == 12)
        #expect(basin.geoJSON.contains("Polygon"))
    }

    @Test("Basin.from leaves a basin without readings unaveraged")
    func basinWithoutReadings() throws {
        let basin = try Basin.from(row: [
            "name": .string("Dry"), "huc2": .string("15"), "geojson": .string("{}"),
            "avgSWE": .null, "stationCount": .int(0),
        ])
        #expect(basin.avgSWE == nil)
        #expect(basin.stationCount == 0)
    }

    @Test("QueryLogEntry exposes a failure's message")
    func logEntryFailure() {
        let entry = QueryLogEntry(cypher: "X", parameters: nil, durationMilliseconds: 0,
                                  queryType: nil, rowCount: 0, mutationCount: 0,
                                  outcome: .failure("boom"))
        #expect(entry.failureMessage == "boom")
        #expect(!entry.succeeded)
    }

    @Test("Basin.from refuses a row without a name")
    func basinMissingName() {
        #expect(throws: RelateError.self) { try Basin.from(row: ["huc2": .string("17")]) }
    }

    @Test("The legend lists every level, no-data last")
    func legendOrder() {
        #expect(SWELevel.allCases.last == .noData)
        #expect(SWELevel.noData.legendLabel == "No data")
    }
}
