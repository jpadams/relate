import Foundation

struct DemoQuery: Identifiable, Hashable {
    let id: String
    let title: String
    let summary: String
    let systemImage: String
    let cypher: String

    static let examples: [DemoQuery] = [
        DemoQuery(
            id: "leaders",
            title: "Snowpack leaders",
            summary: "Traverse readings and rank stations by SWE.",
            systemImage: "trophy.fill",
            cypher: """
            MATCH (s:Station)-[:HAS_READING]->(r:Reading)
            RETURN s.name AS station, s.state AS state,
                   r.swe_inches AS swe
            ORDER BY swe DESC
            LIMIT 8
            """
        ),
        DemoQuery(
            id: "states",
            title: "State rollup",
            summary: "Group graph results and calculate regional averages.",
            systemImage: "chart.bar.fill",
            cypher: """
            MATCH (s:Station)-[:HAS_READING]->(r:Reading)
            RETURN s.state AS state, count(s) AS stations,
                   avg(r.swe_inches) AS avgSWE
            ORDER BY avgSWE DESC
            """
        ),
        DemoQuery(
            id: "basins",
            title: "Basin graph traversal",
            summary: "Follow spatially-derived IN_BASIN relationships.",
            systemImage: "point.3.connected.trianglepath.dotted",
            cypher: """
            MATCH (s:Station)-[:IN_BASIN]->(b:Basin)
            MATCH (s)-[:HAS_READING]->(r:Reading)
            RETURN b.name AS basin, count(s) AS stations,
                   avg(r.swe_inches) AS avgSWE
            ORDER BY avgSWE DESC
            """
        ),
        DemoQuery(
            id: "conditions",
            title: "High snow, cold air",
            summary: "Combine graph traversal with numeric predicates.",
            systemImage: "snowflake",
            cypher: """
            MATCH (s:Station)-[:HAS_READING]->(r:Reading)
            WHERE r.swe_inches >= 20 AND r.temp_avg_f <= 35
            RETURN s.name AS station, s.elevation AS elevation,
                   r.swe_inches AS swe, r.temp_avg_f AS temp
            ORDER BY swe DESC
            """
        ),
    ]
}
