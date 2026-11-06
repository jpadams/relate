import SwiftUI

struct SWELegendView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text("APR 1 SWE")
                .font(.caption2.bold())

            HStack(spacing: 8) {
                Label {
                    Text("station")
                } icon: {
                    Circle().fill(.primary).frame(width: 7, height: 7)
                }
                Label {
                    Text("basin mean")
                } icon: {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(.primary.opacity(0.25))
                        .frame(width: 9, height: 7)
                }
            }
            .font(.system(size: 8))
            .foregroundStyle(.secondary)

            ForEach(SWELevel.allCases, id: \.self) { level in
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(level.color)
                        .frame(width: 15, height: 9)
                        .overlay(RoundedRectangle(cornerRadius: 2).stroke(.white.opacity(0.8), lineWidth: 0.75))
                    Text(level.legendLabel)
                        .font(.caption2)
                }
            }
        }
        .padding(10)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(.white.opacity(0.45), lineWidth: 0.75))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("April first snow water equivalent legend. Colors progress from light blue for zero to ten inches to dark blue for thirty five inches or more.")
    }
}
