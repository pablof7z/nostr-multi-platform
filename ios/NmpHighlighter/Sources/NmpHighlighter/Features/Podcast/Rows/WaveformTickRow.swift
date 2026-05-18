import SwiftUI

/// One 30-second slice of the timeline rendered as either:
/// - a mini-histogram of real audio peaks (when WaveformExtractor has
///   populated the store's `waveformPeaks`), or
/// - a thin time peg when peaks aren't loaded yet (cellular, just-loaded
///   episode, format unsupported).
///
/// The row is purely time-anchored — `t` is the timestamp at the left edge
/// of the slice; the row covers `[t, t + windowSeconds)`. Tapping seeks to
/// the start of the slice.
struct WaveformTickRow: View {
    let t: Double
    let state: TimelineRowState
    let windowSeconds: Double
    let peaks: [Float]
    let onSeek: (Double) -> Void

    var body: some View {
        Button {
            onSeek(t)
        } label: {
            HStack(alignment: .center, spacing: 14) {
                Text(formatTimestamp(t))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(width: 48, alignment: .leading)

                if peaks.isEmpty {
                    timePeg
                } else {
                    waveformBars
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .opacity(state == .future ? 0.55 : 1.0)
        }
        .buttonStyle(.plain)
    }

    private var timePeg: some View {
        Rectangle()
            .fill(Color(.separator))
            .frame(maxWidth: .infinity)
            .frame(height: 1)
    }

    private var waveformBars: some View {
        Canvas { context, size in
            let count = peaks.count
            guard count > 0 else { return }
            let gap: CGFloat = 1.5
            let totalGapWidth = gap * CGFloat(count - 1)
            let barWidth = max(1, (size.width - totalGapWidth) / CGFloat(count))
            let centerY = size.height / 2

            for (idx, peak) in peaks.enumerated() {
                let normalized = CGFloat(min(max(peak, 0.02), 1.0))
                let h = max(2, normalized * size.height)
                let x = CGFloat(idx) * (barWidth + gap)
                let rect = CGRect(
                    x: x,
                    y: centerY - h / 2,
                    width: barWidth,
                    height: h
                )
                context.fill(
                    Path(roundedRect: rect, cornerRadius: barWidth / 2),
                    with: .color(Color.secondary)
                )
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: 18)
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
