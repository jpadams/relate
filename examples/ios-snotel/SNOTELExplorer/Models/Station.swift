import Foundation
import Relate

struct Station: Identifiable, Equatable {
    let id: Int
    let name: String
    let state: String
    let latitude: Double
    let longitude: Double
    let elevation: Int
    var sweInches: Double?
    var distanceMeters: Double?

    var sweLevel: SWELevel { SWELevel.from(swe: sweInches) }

    /// Maps one row of `DatabaseService`'s station projection. Its `id`,
    /// `name`, `state`, `lat`, `lon` and `elevation` columns are guaranteed,
    /// so a row without them is a broken query contract and throws; `swe`
    /// (an OPTIONAL MATCH) and `distance` (nearby searches only) are
    /// genuinely optional.
    static func from(row: [String: CypherValue]) throws -> Station {
        guard let id = row["id"]?.intValue,
              let name = row["name"]?.stringValue,
              let state = row["state"]?.stringValue,
              let lat = row["lat"]?.doubleValue,
              let lon = row["lon"]?.doubleValue,
              let elevation = row["elevation"]?.intValue else {
            throw RelateError.decoding("not a station row: \(row)")
        }
        return Station(id: Int(id), name: name, state: state,
                       latitude: lat, longitude: lon, elevation: Int(elevation),
                       sweInches: row["swe"]?.doubleValue,
                       distanceMeters: row["distance"]?.doubleValue)
    }
}
