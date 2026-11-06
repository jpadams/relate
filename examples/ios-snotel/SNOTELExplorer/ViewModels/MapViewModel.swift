import Foundation
import CoreLocation
import Relate
import Observation

/// Which stations the map shows: exactly one filter is active at a time.
enum StationFilter: Equatable {
    case all
    case basin(String)
    case nearby(CLLocationCoordinate2D)
    case search(String)

    static func == (lhs: StationFilter, rhs: StationFilter) -> Bool {
        switch (lhs, rhs) {
        case (.all, .all): return true
        case let (.basin(a), .basin(b)), let (.search(a), .search(b)): return a == b
        case let (.nearby(a), .nearby(b)): return a.latitude == b.latitude && a.longitude == b.longitude
        default: return false
        }
    }
}

/// Where first-launch setup stands; the app shows the map only when `ready`.
enum SetupPhase: Equatable {
    case loading
    case ready
    case failed(String)
}

@Observable
@MainActor
final class MapViewModel {
    var stations: [Station] = []
    var basins: [Basin] = []
    var databaseStats: DatabaseStats?
    var queryHistory: [QueryLogEntry] = []
    var selectedStation: Station?
    private(set) var filter: StationFilter = .all
    /// The nearby-search radius; a parameter of `.nearby`, kept across searches.
    var radiusKm: Double = 100
    /// The search field's text; it becomes `.search` only on submit.
    var searchText = ""
    private(set) var phase: SetupPhase = .loading
    /// The latest failed station refresh, shown over the ready map.
    var actionError: String?

    private var dbService: DatabaseService?
    private var logConsumer: Task<Void, Never>?

    /// Opens the durable store and has the `DatabaseService` actor load the
    /// bundled data on first launch (later launches reopen the checkpointed
    /// graph), then runs the initial queries. Each `await` yields the
    /// `MainActor`, so the loading `ProgressView` keeps animating; the
    /// `@Observable` state below is only assigned after the awaits resume back
    /// on the `MainActor`.
    func setup(bundle: Bundle? = nil) async {
        guard dbService == nil else { return }
        phase = .loading
        do {
            let svc = try DatabaseService(path: DatabaseService.persistentStoreURL().path)
            consumeQueryLog(of: svc)
            try await svc.loadBundledDataIfNeeded(bundle: bundle)

            let loadedStations = try await svc.allStationsWithSWE()
            let loadedBasins = try await svc.basins()
            let loadedStats = try await svc.databaseStats()

            dbService = svc
            stations = loadedStations
            basins = loadedBasins
            databaseStats = loadedStats
            phase = .ready
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }

    /// Basins with readings, highest mean SWE first: the ranking the filter
    /// panel and the insights chart show.
    var basinsBySWE: [Basin] {
        basins
            .compactMap { basin in basin.avgSWE.map { (basin, $0) } }
            .sorted { $0.1 > $1.1 }
            .map(\.0)
    }

    /// The basin of an active `.basin` filter.
    var selectedBasinName: String? {
        if case .basin(let name) = filter { name } else { nil }
    }

    /// The center of an active `.nearby` filter.
    var nearbyCenter: CLLocationCoordinate2D? {
        if case .nearby(let center) = filter { center } else { nil }
    }

    var isFiltered: Bool { filter != .all }

    func filterByBasin(_ name: String?) async {
        await apply(name.map(StationFilter.basin) ?? .all)
    }

    func searchNearby(center: CLLocationCoordinate2D) async {
        await apply(.nearby(center))
    }

    func showAll() async {
        await apply(.all)
    }

    func searchStations() async {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        await apply(query.isEmpty ? .all : .search(query))
    }

    func runDemoQuery(_ cypher: String) async throws -> QueryResult {
        guard let dbService else {
            throw DemoAppError.databaseNotReady
        }
        return try await dbService.run(cypher)
    }

    func clearQueryHistory() {
        queryHistory.removeAll()
    }

    /// The one station-refresh path: makes `filter` active, fetches its
    /// stations, and surfaces a failure through `actionError`.
    private func apply(_ filter: StationFilter) async {
        self.filter = filter
        if case .search(let text) = filter { searchText = text } else { searchText = "" }
        guard let svc = dbService else { return }
        do {
            switch filter {
            case .all:
                stations = try await svc.allStationsWithSWE()
            case .basin(let name):
                stations = try await svc.stationsInBasin(basinName: name)
            case .nearby(let center):
                stations = try await svc.stationsNearby(
                    lat: center.latitude, lon: center.longitude,
                    radiusMeters: radiusKm * 1000
                )
            case .search(let text):
                stations = try await svc.searchStations(text)
            }
            actionError = nil
        } catch {
            actionError = error.localizedDescription
        }
    }

    /// The one place query-log entries reach observable state: every
    /// statement the service runs, in execution order, for the service's life.
    private func consumeQueryLog(of svc: DatabaseService) {
        logConsumer?.cancel()
        logConsumer = Task { [weak self, log = svc.queryLog] in
            for await entry in log {
                self?.queryHistory.append(entry)
            }
        }
    }
}

private enum DemoAppError: LocalizedError {
    case databaseNotReady

    var errorDescription: String? {
        "The embedded database is still loading."
    }
}
