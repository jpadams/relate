import Foundation

/// The SWE classification shared by station symbols and basin averages. Its
/// color ramp lives with the views (`Views/SWELevel+Color.swift`), so the core
/// library stays free of UI frameworks. Cases are declared in legend order.
enum SWELevel: String, CaseIterable {
    case low = "Low"
    case moderate = "Moderate"
    case high = "High"
    case veryHigh = "Very High"
    case noData = "No Data"

    var legendLabel: String {
        switch self {
        case .low: return "0-10\""
        case .moderate: return "10-20\""
        case .high: return "20-35\""
        case .veryHigh: return "35+\""
        case .noData: return "No data"
        }
    }

    static func from(swe: Double?) -> SWELevel {
        guard let swe else { return .noData }
        switch swe {
        case ..<10: return .low
        case 10..<20: return .moderate
        case 20..<35: return .high
        default: return .veryHigh
        }
    }
}
