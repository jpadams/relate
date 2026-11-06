import SwiftUI

extension SWELevel {
    var color: Color {
        // A colorblind-safe, monotonically darkening ice-to-deep-water ramp.
        // The same ramp is used for station symbols and basin averages.
        switch self {
        case .noData: return Color(red: 0.49, green: 0.52, blue: 0.56)
        case .low: return Color(red: 0.84, green: 0.94, blue: 0.96)
        case .moderate: return Color(red: 0.47, green: 0.78, blue: 0.83)
        case .high: return Color(red: 0.18, green: 0.56, blue: 0.75)
        case .veryHigh: return Color(red: 0.19, green: 0.30, blue: 0.61)
        }
    }
}
