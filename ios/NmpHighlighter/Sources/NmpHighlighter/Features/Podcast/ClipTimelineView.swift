import SwiftUI

/// Horizontal scrub bar + clip-range selector. The whole strip is tappable
/// to seek; the scrub thumb and each clip boundary can be dragged.
struct ClipTimelineView: View {
    @Binding var clipStart: TimeInterval?
    @Binding var clipEnd: TimeInterval?
    @Binding var currentTime: TimeInterval
    let duration: TimeInterval
    let loadedTimeRanges: [ClosedRange<TimeInterval>]
    let onSeek: (TimeInterval) -> Void

    private let trackHeight: CGFloat = 6
    private let scrubSize: CGFloat = 20
    private let clipThumbHeight: CGFloat = 22
    private let clipThumbWidth: CGFloat = 10

    // Captures the original boundary value at the start of a thumb drag so
    // onChanged deltas compose against it rather than the live binding.
    @State private var dragStartBaseline: TimeInterval?
    @State private var dragEndBaseline: TimeInterval?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            GeometryReader { proxy in
                let width = proxy.size.width
                ZStack(alignment: .leading) {
                    baseTrack
                    bufferedFills(width: width)
                    clipRangeFill(width: width)
                    scrubThumb(width: width)
                    if let start = clipStart {
                        startThumb(at: position(for: start, width: width), width: width)
                    }
                    if let end = clipEnd {
                        endThumb(at: position(for: end, width: width), width: width)
                    }
                }
                .contentShape(Rectangle())
                .gesture(trackTapGesture(width: width))
            }
            .frame(height: max(scrubSize, clipThumbHeight) + 4)

            HStack {
                Text(formatTime(currentTime))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Spacer()
                Text(formatTime(duration))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Subviews

    @ViewBuilder
    private func bufferedFills(width: CGFloat) -> some View {
        if duration > 0 {
            ForEach(Array(loadedTimeRanges.enumerated()), id: \.offset) { _, range in
                let x1 = position(for: range.lowerBound, width: width)
                let x2 = position(for: range.upperBound, width: width)
                Capsule()
                    .fill(Color.secondary.opacity(0.35))
                    .frame(width: max(2, x2 - x1), height: trackHeight)
                    .offset(x: x1)
                    .frame(maxHeight: .infinity)
            }
        }
    }

    private var baseTrack: some View {
        Capsule()
            .fill(Color.secondary.opacity(0.25))
            .frame(height: trackHeight)
            .frame(maxHeight: .infinity)
    }

    @ViewBuilder
    private func clipRangeFill(width: CGFloat) -> some View {
        if let start = clipStart, let end = clipEnd, duration > 0, end > start {
            let x1 = position(for: start, width: width)
            let x2 = position(for: end, width: width)
            Capsule()
                .fill(Color.highlighterAccent.opacity(0.4))
                .frame(width: max(2, x2 - x1), height: trackHeight)
                .offset(x: x1)
                .frame(maxHeight: .infinity)
        }
    }

    private func scrubThumb(width: CGFloat) -> some View {
        Circle()
            .fill(Color.primary)
            .frame(width: scrubSize, height: scrubSize)
            .shadow(radius: 1, y: 1)
            .offset(x: position(for: currentTime, width: width) - scrubSize / 2)
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        onSeek(time(for: value.location.x, width: width))
                    }
            )
    }

    private func startThumb(at x: CGFloat, width: CGFloat) -> some View {
        clipThumb()
            .offset(x: x - clipThumbWidth / 2)
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        let base = dragStartBaseline ?? clipStart ?? 0
                        if dragStartBaseline == nil { dragStartBaseline = base }
                        let delta = Double(value.translation.width) / Double(max(1, width)) * duration
                        let candidate = clampTime(base + delta)
                        let maxStart = (clipEnd ?? duration) - 0.05
                        clipStart = min(candidate, maxStart > 0 ? maxStart : candidate)
                    }
                    .onEnded { _ in dragStartBaseline = nil }
            )
    }

    private func endThumb(at x: CGFloat, width: CGFloat) -> some View {
        clipThumb()
            .offset(x: x - clipThumbWidth / 2)
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        let base = dragEndBaseline ?? clipEnd ?? 0
                        if dragEndBaseline == nil { dragEndBaseline = base }
                        let delta = Double(value.translation.width) / Double(max(1, width)) * duration
                        let candidate = clampTime(base + delta)
                        let minEnd = (clipStart ?? 0) + 0.05
                        clipEnd = max(candidate, minEnd)
                    }
                    .onEnded { _ in dragEndBaseline = nil }
            )
    }

    private func clipThumb() -> some View {
        RoundedRectangle(cornerRadius: 2)
            .fill(Color.highlighterAccent)
            .frame(width: clipThumbWidth, height: clipThumbHeight)
            .overlay(
                RoundedRectangle(cornerRadius: 2)
                    .stroke(Color.white.opacity(0.6), lineWidth: 1)
            )
    }

    private func trackTapGesture(width: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onEnded { value in
                if abs(value.translation.width) < 1 && abs(value.translation.height) < 1 {
                    onSeek(time(for: value.location.x, width: width))
                }
            }
    }

    // MARK: - Math

    private func position(for seconds: TimeInterval, width: CGFloat) -> CGFloat {
        guard duration > 0 else { return 0 }
        let ratio = max(0, min(1, seconds / duration))
        return width * CGFloat(ratio)
    }

    private func time(for x: CGFloat, width: CGFloat) -> TimeInterval {
        guard duration > 0, width > 0 else { return 0 }
        let ratio = max(0, min(1, x / width))
        return duration * Double(ratio)
    }

    private func clampTime(_ value: TimeInterval) -> TimeInterval {
        if duration <= 0 { return max(0, value) }
        return max(0, min(duration, value))
    }

    private func formatTime(_ seconds: TimeInterval) -> String {
        guard seconds.isFinite, seconds >= 0 else { return "00:00" }
        let total = Int(seconds)
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let secs = total % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, secs)
        }
        return String(format: "%02d:%02d", minutes, secs)
    }
}
