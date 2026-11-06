import Foundation
import Relate

struct Basin: Identifiable, Equatable {
    let id: String
    let name: String
    let huc2: String
    /// The basin's GeoJSON geometry object, for the map overlay.
    let geoJSON: String
    /// Mean SWE of the basin's station readings; nil when it has none.
    let avgSWE: Double?
    /// How many of the basin's stations report a reading.
    let stationCount: Int

    /// Maps one row of `DatabaseService.basins()`. Every column but `avgSWE`
    /// (null for a basin without readings) is guaranteed, so a row without
    /// one throws.
    static func from(row: [String: CypherValue]) throws -> Basin {
        guard let name = row["name"]?.stringValue,
              let huc2 = row["huc2"]?.stringValue,
              let geoJSON = row["geojson"]?.stringValue,
              let stationCount = row["stationCount"]?.intValue else {
            throw RelateError.decoding("not a basin row: \(row)")
        }
        return Basin(id: huc2, name: name, huc2: huc2, geoJSON: geoJSON,
                     avgSWE: row["avgSWE"]?.doubleValue,
                     stationCount: Int(stationCount))
    }
}
