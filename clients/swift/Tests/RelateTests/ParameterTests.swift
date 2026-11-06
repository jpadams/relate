import Foundation
import Testing

@testable import Relate

@Suite struct ParameterTests {
    let conn = try! Database.inMemory().connect()

    @Test func scalarsBind() throws {
        let r = try conn.query(
            "RETURN $s AS s, $big + 0 AS big, $f AS f, $empty AS empty, $n IS NULL AS isNull",
            ["s": "hé", "big": 9_007_199_254_740_993, "f": .float(.nan), "empty": "", "n": nil])
        #expect(r.rows[0][0] == "hé")
        #expect(r.rows[0][1] == .int(9_007_199_254_740_993))
        #expect(r.rows[0][2].doubleValue?.isNaN == true)
        #expect(r.rows[0][3] == .string(""))
        #expect(r.rows[0][4] == .bool(true))
    }

    @Test func anInteriorNULInParameterValuesCrossesWhole() throws {
        let r = try conn.query("RETURN size($s) AS n", ["s": .string("a\u{0}b")])
        #expect(try r.scalar() == 3)
    }

    @Test func containersBind() throws {
        let r = try conn.query(
            "RETURN $l AS l, $m.k AS k, $ll[0] AS ll",
            ["l": [1.5, nil], "m": ["k": true], "ll": [[1, 2]]])
        #expect(r.rows[0][0] == [1.5, nil])
        #expect(r.rows[0][1] == true)
        #expect(r.rows[0][2] == [1, 2])
    }

    @Test func typedValuesBindDirectly() throws {
        let params: [String: CypherValue] = [
            "d": .date(.init(year: 2026, month: 1, day: 2)),
            "t": .time(.init(hour: 9, minute: 30, second: 0, nanosecond: 0, offsetSeconds: 3600)),
            "dt": .dateTime(.init(
                year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0,
                offsetSeconds: 0, zone: "Europe/Berlin")),
            "du": .duration(.init(months: 14, days: 0, seconds: 36 * 3600, nanoseconds: 0)),
            "p": .point(.init(srid: 4326, x: -105.0, y: 40.0, z: nil)),
            "v": .vector(.ints(dtype: .integer8, values: [1, 2, 3])),
        ]
        let r = try conn.query(
            "RETURN $d AS d, $t AS t, $dt AS dt, $du AS du, $p AS p, $v AS v", params)
        #expect(r.rows[0][0] == .date(.init(year: 2026, month: 1, day: 2)))
        #expect(r.rows[0][1] == .time(
            .init(hour: 9, minute: 30, second: 0, nanosecond: 0, offsetSeconds: 3600)))
        #expect(r.rows[0][2] == .dateTime(.init(
            year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0,
            offsetSeconds: 3600, zone: "Europe/Berlin")))
        #expect(r.rows[0][3] == .duration(
            .init(months: 14, days: 0, seconds: 36 * 3600, nanoseconds: 0)))
        #expect(r.rows[0][4] == .point(.init(srid: 4326, x: -105.0, y: 40.0, z: nil)))
        #expect(r.rows[0][5] == .vector(.ints(dtype: .integer8, values: [1, 2, 3])))
    }

    @Test func entitiesAreRefusedAsParameters() throws {
        let node = try #require(
            try conn.query("CREATE (n) RETURN n").rows[0][0].nodeValue)
        #expect(throws: RelateError.self) {
            try conn.query("RETURN $n", ["n": .node(node)])
        }
        let path = try #require(try conn.query("CREATE p = (:A)-[:T]->(:B) RETURN p").rows[0][0].pathValue)
        #expect(throws: RelateError.self) {
            try conn.query("RETURN $p", ["p": .path(path)])
        }
        #expect(throws: RelateError.self) {
            try conn.query("RETURN 1\u{0}")
        }
    }

    @Test func aPutterRefusalIsTheTypedErrorShape() throws {
        // A parameter the wrapper can encode but the engine's builder
        // refuses (month 13 is not a calendar month) crosses the error
        // channel with the internal stage.
        let error = try #require(throws: RelateError.self) {
            try conn.query(
                "RETURN $d", ["d": .date(.init(year: 2026, month: 13, day: 2))])
        }
        let engine = try #require(error.engineError)
        #expect(engine.stage == .internal && !engine.retryable)
    }
}
