import SwiftUI

struct ChapterRow: View {
    let t: Double
    let title: String
    let state: TimelineRowState
    let onSeek: (Double) -> Void

    var body: some View {
        Button {
            onSeek(t)
        } label: {
            HStack(alignment: .center, spacing: 14) {
                Text(formatTimestamp(t))
                    .font(.caption.weight(.semibold).monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(width: 48, alignment: .leading)

                VStack(alignment: .leading, spacing: 0) {
                    Rectangle()
                        .fill(Color(.separator))
                        .frame(maxWidth: .infinity, maxHeight: 2)
                    Text(title)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(state == .future ? Color.secondary : Color.primary)
                        .padding(.top, 6)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .opacity(state == .future ? 0.55 : 1.0)
        }
        .buttonStyle(.plain)
    }
}

private func formatTimestamp(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds >= 0 else { return "0:00" }
    let total = Int(seconds)
    let h = total / 3600
    let m = (total % 3600) / 60
    let s = total % 60
    if h > 0 { return String(format: "%d:%02d:%02d", h, m, s) }
    return String(format: "%d:%02d", m, s)
}
