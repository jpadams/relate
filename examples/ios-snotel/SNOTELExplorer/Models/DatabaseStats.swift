import Foundation

struct DatabaseStats: Equatable {
    let stations: Int
    let readings: Int
    let basins: Int
    let relationships: Int
    let basinMemberships: Int

    var nodes: Int { stations + readings + basins }
}
