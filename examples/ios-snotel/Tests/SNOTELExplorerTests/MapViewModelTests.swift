import Testing
import Foundation
@testable import SNOTELCore

@Suite("MapViewModel")
@MainActor
struct MapViewModelTests {

    @Test("setup() surfaces a missing-resource failure as the failed phase")
    func surfacesLoaderError() async throws {
        let emptyBundle = try makeEmptyBundle()
        let viewModel = MapViewModel()
        await viewModel.setup(bundle: emptyBundle)

        guard case .failed(let message) = viewModel.phase else {
            Issue.record("Expected the failed phase, got \(viewModel.phase)")
            return
        }
        #expect(message.contains("snotel_stations.csv"))
        #expect(viewModel.actionError == nil)
    }
}
