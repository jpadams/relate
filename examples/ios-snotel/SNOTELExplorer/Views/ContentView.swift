import SwiftUI

struct ContentView: View {
    @State private var viewModel = MapViewModel()
    @State private var showInsights = false
    @State private var showWorkbench = false
    @State private var showQueryLog = false

    var body: some View {
        ZStack(alignment: .bottom) {
            switch viewModel.phase {
            case .loading:
                ProgressView("Loading SNOTEL data...")
            case .failed(let error):
                VStack(spacing: 16) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.largeTitle)
                        .foregroundStyle(.red)
                    Text(error)
                        .multilineTextAlignment(.center)
                        .padding()
                    Button("Try Again") {
                        Task { await viewModel.setup() }
                    }
                    .buttonStyle(.borderedProminent)
                }
            case .ready:
                MapContainerView(viewModel: viewModel)
                    .ignoresSafeArea()

                VStack(spacing: 0) {
                    HStack {
                        VStack(spacing: 10) {
                            MapActionButton(
                                title: "Graph insights",
                                systemImage: "chart.xyaxis.line",
                                action: { showInsights = true }
                            )
                            MapActionButton(
                                title: "Cypher workbench",
                                systemImage: "terminal.fill",
                                action: { showWorkbench = true }
                            )
                            MapActionButton(
                                title: "Query history",
                                systemImage: "list.bullet.rectangle.portrait",
                                action: { showQueryLog.toggle() }
                            )
                        }
                        .padding(.leading, 12)
                        Spacer()
                        SWELegendView()
                            .padding(.trailing, 12)
                    }
                    .padding(.top, 60)
                    Spacer()
                }

                VStack(spacing: 0) {
                    Spacer()
                    FilterPanelView(viewModel: viewModel)
                        .background(.ultraThinMaterial)
                        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                        .padding(.horizontal, 8)
                        .padding(.bottom, 8)
                }
            }
        }
        .task {
            await viewModel.setup()
        }
        .alert(
            "Unable to update stations",
            isPresented: Binding(
                get: { viewModel.actionError != nil },
                set: { if !$0 { viewModel.actionError = nil } }
            )
        ) {
            Button("OK", role: .cancel) { viewModel.actionError = nil }
        } message: {
            Text(viewModel.actionError ?? "Unknown error")
        }
        .sheet(item: $viewModel.selectedStation) { station in
            StationDetailSheet(station: station, viewModel: viewModel)
                .presentationDetents([.medium])
        }
        .sheet(isPresented: $showInsights) {
            if let stats = viewModel.databaseStats {
                InsightsView(stats: stats, basinsBySWE: viewModel.basinsBySWE)
            }
        }
        .sheet(isPresented: $showWorkbench) {
            QueryWorkbenchView(viewModel: viewModel)
        }
        .sheet(isPresented: $showQueryLog) {
            QueryLogView(viewModel: viewModel)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
    }
}

private struct MapActionButton: View {
    let title: String
    let systemImage: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.headline)
                .frame(width: 42, height: 42)
                .background(.ultraThinMaterial, in: Circle())
                .overlay(Circle().stroke(.white.opacity(0.55), lineWidth: 1))
                .shadow(color: .black.opacity(0.12), radius: 5, y: 2)
        }
        .foregroundStyle(.primary)
        .accessibilityLabel(title)
    }
}
