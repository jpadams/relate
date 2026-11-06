import Foundation
import Testing

@testable import Relate

@Suite struct ValueTests {
    let conn = try! Database.inMemory().connect()

    @Test func scalarsRoundTrip() throws {
        let r = try conn.query(
            "RETURN 1 AS i, 1.0 AS f, true AS b, 'x' AS s, null AS n, [1, 'a'] AS l, {k: 2} AS m")
        #expect(r.rows[0] == [.int(1), .float(1.0), .bool(true), .string("x"), .null,
                              [1, "a"], ["k": 2]])
        #expect(r.rows[0].map(\.kind) == [.int, .float, .bool, .string, .null, .list, .map])
    }

    @Test func int64Extremes() throws {
        let r = try conn.query("RETURN 9223372036854775807 AS hi, -9223372036854775807 - 1 AS lo")
        #expect(r.rows[0] == [.int(.max), .int(.min)])
    }

    @Test func nonFiniteFloatsStayFloats() throws {
        let r = try conn.query("RETURN 1.0/0.0 AS inf, -1.0/0.0 AS ninf, 0.0/0.0 AS nan")
        #expect(r.rows[0].map(\.kind) == [.float, .float, .float])
        let values = r.rows[0].compactMap(\.doubleValue)
        #expect(values[0] == .infinity && values[1] == -.infinity && values[2].isNaN)
    }

    @Test func interiorNULCrossesWhole() throws {
        let r = try conn.query("RETURN 'a\\u0000b' AS s")
        #expect(try r.scalar() == .string("a\u{0}b"))
    }

    @Test func extremeYearsRenderWithoutTrapping() throws {
        // description zero-pads the year; a negative repeat count would trap.
        let r = try conn.query("RETURN date({year: -12345, month: 1, day: 2}) AS a, date({year: 12345, month: 1, day: 2}) AS b, date({year: -5, month: 1, day: 2}) AS c")
        #expect(r.rows[0].map(\.description) == ["-12345-01-02", "12345-01-02", "-0005-01-02"])
    }

    @Test func mapKeyWithInteriorNULIsRefusedNotTruncated() throws {
        // map_get reads keys NUL-terminated, so 'a\0b' would otherwise
        // resolve to the 'a' entry's value.
        #expect(throws: RelateError.self) {
            _ = try conn.query("RETURN {'a\\u0000b': 1, a: 2} AS m")
        }
    }

    @Test func nodesAndRelationshipsCarryElementIds() throws {
        let r = try conn.query("CREATE (a:A {x: 1})-[t:T {w: 2}]->(b:B) RETURN a, t, b")
        #expect(r.rows[0].map(\.kind) == [.node, .relationship, .node])
        let a = try #require(r.rows[0][0].nodeValue)
        let t = try #require(r.rows[0][1].relationshipValue)
        let b = try #require(r.rows[0][2].nodeValue)
        #expect(a.labels == ["A"] && a.properties == ["x": 1])
        #expect(t.type == "T" && t.startElementId == a.elementId && t.endElementId == b.elementId)
        #expect(t.properties == ["w": 2])
        #expect(r.rows[0][1]["w"] == 2)
        // The element id is Cypher's own spelling, so it round-trips as a
        // parameter: WHERE elementId(n) = $id.
        let again = try conn.query(
            "MATCH (n) WHERE elementId(n) = $id RETURN labels(n) AS labels",
            ["id": .string(a.elementId)])
        #expect(try again.scalar() == ["A"])
    }

    @Test func entitiesCarryTheNumericIdsCypherIdReturns() throws {
        // ADR-0506: the numeric ids are exactly the INTEGERs Cypher's
        // id()/id(startNode(r))/id(endNode(r)) return for the same entities.
        let r = try conn.query(
            "CREATE (a:A {x: 1})-[t:T {w: 2}]->(b:B)"
                + " RETURN a, t, b, id(a) AS ia, id(t) AS it, id(startNode(t)) AS isA, id(endNode(t)) AS ieA")
        let a = try #require(r.rows[0][0].nodeValue)
        let t = try #require(r.rows[0][1].relationshipValue)
        let b = try #require(r.rows[0][2].nodeValue)
        let ia = try #require(r.rows[0][3].intValue)
        let it = try #require(r.rows[0][4].intValue)
        let start = try #require(r.rows[0][5].intValue)
        let end = try #require(r.rows[0][6].intValue)
        #expect(a.id == ia)
        #expect(b.id == end)
        #expect(t.id == it)
        #expect(t.startId == start && t.startId == a.id)
        #expect(t.endId == end && t.endId == b.id)
        // The numeric ids also cross at depth: a path's entities carry them.
        let p = try #require(try conn.query(
            "MATCH p = (a:A)-[t:T]->(b:B) RETURN p").rows[0][0].pathValue)
        #expect(p.nodes[0].id == ia && p.relationships[0].id == it && p.nodes[1].id == end)
    }

    @Test func aMapThatLooksLikeANodeStaysAMap() throws {
        let r = try conn.query("RETURN {id: 1, labels: ['A'], properties: {}} AS m")
        #expect(try r.scalar().kind == .map)
        #expect(r.rows[0][0].nodeValue == nil)
    }

    @Test func aPresentNullIsNotAnAbsentKey() throws {
        let r = try conn.query("RETURN {a: null} AS withNull, {} AS empty")
        let withNull = try #require(r.rows[0][0].mapValue)
        let empty = try #require(r.rows[0][1].mapValue)
        #expect(withNull["a"] == .null)
        #expect(empty["a"] == nil)
    }

    @Test func pathIsItsOwnKind() throws {
        let r = try conn.query("CREATE p = (:A)-[:T]->(:B) RETURN p")
        let path = try #require(try r.scalar().pathValue)
        #expect(path.nodes.count == 2 && path.relationships.count == 1)
        #expect(path.nodes[0].labels == ["A"] && path.nodes[1].labels == ["B"])
        #expect(path.relationships[0].type == "T")
        #expect(path.nodes[0].elementId == path.relationships[0].startElementId)
        // Structural discrimination at every depth: a path nested in a
        // list is still a path, not a plain list.
        let nested = try conn.query("MATCH p = (:A)-[:T]->(:B) RETURN [p] AS ps")
        let items = try #require(try nested.scalar().listValue)
        #expect(items.count == 1 && items[0].kind == .path)
    }

    @Test func temporalsDecodeAsComponents() throws {
        let r = try conn.query(
            """
            RETURN date('2026-01-02') AS d,
                   time('09:30:05.000000123+01:00') AS t,
                   localtime('09:30:05') AS lt,
                   datetime({year: 2026, month: 11, day: 4, timezone: 'Europe/Berlin'}) AS dt,
                   localdatetime('2026-01-02T03:04:05.000000123') AS ldt,
                   duration('P1Y2M') AS du
            """)
        #expect(r.rows[0].map(\.kind) == [.date, .time, .localTime, .dateTime, .localDateTime, .duration])
        #expect(r.rows[0][0] == .date(.init(year: 2026, month: 1, day: 2)))
        #expect(r.rows[0][1] == .time(.init(
            hour: 9, minute: 30, second: 5, nanosecond: 123, offsetSeconds: 3600)))
        #expect(r.rows[0][2] == .localTime(.init(hour: 9, minute: 30, second: 5, nanosecond: 0)))
        #expect(r.rows[0][3] == .dateTime(.init(
            year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0,
            offsetSeconds: 3600, zone: "Europe/Berlin")))
        #expect(r.rows[0][4] == .localDateTime(.init(
            year: 2026, month: 1, day: 2, hour: 3, minute: 4, second: 5, nanosecond: 123)))
        #expect(r.rows[0][5] == .duration(.init(months: 14, days: 0, seconds: 0, nanoseconds: 0)))
    }

    @Test func anOffsetOnlyDatetimeCarriesNoZone() throws {
        let r = try conn.query("RETURN datetime('2026-11-04T09:30:00+01:00') AS dt")
        let dt = try #require(try r.scalar().dateTimeValue)
        #expect(dt.zone == nil && dt.offsetSeconds == 3600 && dt.hour == 9)
    }

    @Test func pointsAndVectorsDecodeTyped() throws {
        let r = try conn.query(
            "RETURN point({longitude: -105.0, latitude: 40.0}) AS p,"
                + " point({x: 1, y: 2, z: 3}) AS q,"
                + " vector([1, 2, 3], 3, INTEGER8) AS iv,"
                + " vector([0.25, -1.0, 0.5], 3, FLOAT32) AS fv")
        #expect(r.rows[0].map(\.kind) == [.point, .point, .vector, .vector])
        #expect(r.rows[0][0] == .point(.init(srid: 4326, x: -105.0, y: 40.0, z: nil)))
        #expect(r.rows[0][1] == .point(.init(srid: 9157, x: 1, y: 2, z: 3)))
        #expect(r.rows[0][2] == .vector(.ints(dtype: .integer8, values: [1, 2, 3])))
        #expect(r.rows[0][3] == .vector(.floats(dtype: .float32, values: [0.25, -1.0, 0.5])))
    }
}
