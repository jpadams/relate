import CRelate

// MARK: - Result materialization (ADR-0502 §1)

extension QueryResult {
    /// Reads one good result into a `QueryResult` before it is destroyed:
    /// cells through the typed walker, the metadata documents from their
    /// JSON. The boundary decode lives here, not in the model file,
    /// mirroring `CypherValue+Walker.swift`.
    static func materialize(_ result: OpaquePointer) throws -> QueryResult {
        let columnCount = Int(relate_column_count(result))
        let rowCount = Int(relate_row_count(result))
        var columns: [String] = []
        columns.reserveCapacity(columnCount)
        for col in 0..<columnCount {
            columns.append(try required(relate_column_name(result, Int64(col)), "column name \(col)"))
        }
        var rows: [[CypherValue]] = []
        rows.reserveCapacity(rowCount)
        for row in 0..<rowCount {
            var values: [CypherValue] = []
            values.reserveCapacity(columnCount)
            for col in 0..<columnCount {
                values.append(
                    try CypherValue.decode(relate_value_at(result, Int64(row), Int64(col))))
            }
            rows.append(values)
        }
        let queryTypeName = try required(relate_result_query_type(result), "query type")
        guard let queryType = QueryType(rawValue: queryTypeName) else {
            throw RelateError.decoding("unknown query type '\(queryTypeName)'")
        }
        let plan = try relate_result_plan_json(result).map {
            try decodeMetadata(PlanDocument.self, try required($0, "the plan document"), what: "plan")
        }
        return QueryResult(
            columns: columns,
            rows: rows,
            bookmark: try relate_result_bookmark(result).map {
                try required($0, "the result's bookmark")
            },
            queryType: queryType,
            namespace: try required(relate_result_namespace(result), "namespace"),
            stats: try decodeMetadata(
                QueryStats.self, try required(relate_result_stats(result), "stats"), what: "stats"),
            notifications: try decodeMetadata(
                [Notification].self,
                try required(relate_result_notifications(result), "notifications"),
                what: "notifications"),
            plan: plan,
            elapsedMs: relate_result_elapsed_ms(result),
            parseMs: relate_result_parse_ms(result),
            planMs: relate_result_plan_ms(result),
            executeMs: relate_result_execute_ms(result))
    }
}
