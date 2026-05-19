import SwiftUI

struct TranscriptRow: View {
    let segment: TranscriptSegment
    let state: TimelineRowState
    let onSeek: (Double) -> Void

    var body: some View {
        Button {
            onSeek(segment.start)
        } label: {
            HStack(alignment: .top, spacing: 14) {
                Text(formatTimestamp(segment.start))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(width: 48, alignment: .leading)
                    .padding(.top, 1)

                VStack(alignment: .leading, spacing: 4) {
                    if !segment.speaker.isEmpty {
                        Text(segment.speaker)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.secondary)
                    }
                    Text(segment.text)
                        .font(.system(size: 15))
                        .lineSpacing(15 * 0.55)
                        .foregroundStyle(.primary)
                        .multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                state == .active
                    ? Color(.separator).opacity(0.3)
                    : Color.clear
            )
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
