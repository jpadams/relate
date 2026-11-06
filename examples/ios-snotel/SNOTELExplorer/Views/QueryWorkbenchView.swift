import Relate
import SwiftUI

struct QueryWorkbenchView: View {
    let viewModel: MapViewModel
    @Environment(\.dismiss) private var dismiss
    @State private var selectedQuery = DemoQuery.examples[0]
    @State private var queryText = DemoQuery.examples[0].cypher
    @State private var output: DemoQueryOutput?
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    Text("Choose an example, edit its Cypher, then execute it directly against the in-memory graph.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)

                    queryPicker
                    editor
                    executeButton

                    if let output {
                        results(output)
                    }
                }
                .padding()
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("Cypher Workbench")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Reset") {
                        queryText = selectedQuery.cypher
                        output = nil
                    }
                }
            }
            .alert("Query failed", isPresented: Binding(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )) {
                Button("OK", role: .cancel) { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "Unknown error")
            }
            .task { await execute() }
        }
    }

    private var queryPicker: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(DemoQuery.examples) { query in
                    Button {
                        selectedQuery = query
                        queryText = query.cypher
                        output = nil
                    } label: {
                        VStack(alignment: .leading, spacing: 8) {
                            Image(systemName: query.systemImage)
                                .font(.title3)
                            Text(query.title)
                                .font(.subheadline.bold())
                            Text(query.summary)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                        .multilineTextAlignment(.leading)
                        .frame(width: 170, height: 105, alignment: .topLeading)
                        .padding(12)
                        .background(
                            selectedQuery.id == query.id ? Color.blue.opacity(0.14) : Color(.secondarySystemGroupedBackground),
                            in: RoundedRectangle(cornerRadius: 16, style: .continuous)
                        )
                        .overlay {
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .stroke(selectedQuery.id == query.id ? .blue : .clear, lineWidth: 1.5)
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var editor: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Cypher", systemImage: "chevron.left.forwardslash.chevron.right")
                .font(.headline)
            TextEditor(text: $queryText)
                .font(.system(.footnote, design: .monospaced))
                .foregroundStyle(.green)
                .scrollContentBackground(.hidden)
                .padding(10)
                .frame(minHeight: 180)
                .background(Color(red: 0.055, green: 0.075, blue: 0.1), in: RoundedRectangle(cornerRadius: 14))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
    }

    private var executeButton: some View {
        Button {
            Task { await execute() }
        } label: {
            Label("Run query", systemImage: "play.fill")
                .font(.headline)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
        }
        .buttonStyle(.borderedProminent)
        .disabled(queryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }

    private func results(_ output: DemoQueryOutput) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Label("Results", systemImage: "tablecells")
                    .font(.headline)
                Spacer()
                Text("\(output.rowCount) rows • \(output.elapsedMilliseconds, format: .number.precision(.fractionLength(2))) ms")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }

            ScrollView(.horizontal, showsIndicators: true) {
                Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 10) {
                    GridRow {
                        ForEach(output.columns, id: \.self) { column in
                            Text(column)
                                .font(.caption.bold())
                                .foregroundStyle(.blue)
                        }
                    }
                    Divider()
                    ForEach(Array(output.rows.enumerated()), id: \.offset) { _, row in
                        GridRow {
                            ForEach(Array(row.enumerated()), id: \.offset) { _, value in
                                Text(value)
                                    .font(.caption.monospaced())
                                    .lineLimit(2)
                            }
                        }
                    }
                }
                .padding(14)
            }
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14))
        }
    }

    @MainActor
    private func execute() async {
        do {
            output = DemoQueryOutput(try await viewModel.runDemoQuery(queryText))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

/// A workbench result rendered for display: the presentation policy for
/// `CypherValue`s lives here, beside its one consumer.
struct DemoQueryOutput {
    let columns: [String]
    let rows: [[String]]
    let elapsedMilliseconds: Double

    var rowCount: Int { rows.count }

    init(_ result: QueryResult) {
        columns = result.columns
        rows = result.rows.map { $0.map(Self.display) }
        elapsedMilliseconds = result.elapsedMs
    }

    private static func display(_ value: CypherValue) -> String {
        switch value {
        case .null: return "—"
        case .float(let value):
            return value.rounded() == value
                ? String(format: "%.0f", value)
                : String(format: "%.2f", value)
        case .string(let value): return value
        // Every other case renders via its CypherValue description.
        default: return value.description
        }
    }
}
