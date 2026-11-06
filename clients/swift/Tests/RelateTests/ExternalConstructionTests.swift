// An external module's view of the value model: this file deliberately
// imports Relate WITHOUT @testable, so every constructor it spells must
// be truly public — the spec's typed-parameter binding (spec.md
// §Parameters) is only real if it compiles from outside the package, which
// @testable would mask.
import Relate
import Testing

@Suite struct ExternalConstructionTests {
    let conn = try! Database.inMemory().connect()

    @Test func everyTypedParameterValueConstructsAndBindsFromOutsideThePackage() throws {
        let params: [String: CypherValue] = [
            "d": .date(CypherValue.Date(year: 2026, month: 1, day: 2)),
            "t": .time(CypherValue.Time(
                hour: 9, minute: 30, second: 0, nanosecond: 0, offsetSeconds: 3600)),
            "lt": .localTime(CypherValue.LocalTime(
                hour: 9, minute: 30, second: 0, nanosecond: 0)),
            "dt": .dateTime(CypherValue.DateTime(
                year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0,
                offsetSeconds: 0, zone: "Europe/Berlin")),
            "ldt": .localDateTime(CypherValue.LocalDateTime(
                year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0)),
            "du": .duration(CypherValue.Duration(
                months: 14, days: 0, seconds: 36 * 3600, nanoseconds: 0)),
            "p": .point(CypherValue.Point(srid: 4326, x: -105.0, y: 40.0, z: nil)),
            "p3": .point(CypherValue.Point(srid: 4979, x: -105.0, y: 40.0, z: 1234.5)),
        ]
        let r = try conn.query(
            """
            RETURN $d AS d, $t AS t, $lt AS lt, $dt AS dt, $ldt AS ldt, $du AS du,
                   $p AS p, $p3 AS p3
            """,
            params)
        #expect(r.rows[0][0] == .date(CypherValue.Date(year: 2026, month: 1, day: 2)))
        #expect(r.rows[0][1] == .time(CypherValue.Time(
            hour: 9, minute: 30, second: 0, nanosecond: 0, offsetSeconds: 3600)))
        #expect(r.rows[0][2] == .localTime(
            CypherValue.LocalTime(hour: 9, minute: 30, second: 0, nanosecond: 0)))
        #expect(r.rows[0][3] == .dateTime(CypherValue.DateTime(
            year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0,
            offsetSeconds: 3600, zone: "Europe/Berlin")))
        #expect(r.rows[0][4] == .localDateTime(CypherValue.LocalDateTime(
            year: 2026, month: 11, day: 4, hour: 0, minute: 0, second: 0, nanosecond: 0)))
        #expect(r.rows[0][5] == .duration(CypherValue.Duration(
            months: 14, days: 0, seconds: 36 * 3600, nanoseconds: 0)))
        #expect(r.rows[0][6] == .point(CypherValue.Point(srid: 4326, x: -105.0, y: 40.0, z: nil)))
        #expect(r.rows[0][7] == .point(
            CypherValue.Point(srid: 4979, x: -105.0, y: 40.0, z: 1234.5)))
    }
}
