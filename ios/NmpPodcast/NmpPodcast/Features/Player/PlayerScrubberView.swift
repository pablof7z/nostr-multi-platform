import SwiftUI

// MARK: - PlayerScrubberView

/// Timeline scrubber + clock readout used by the full-screen `PlayerView`.
///
/// Embeds `PlayerTimelineView` (chapter ticks, clip highlights, playhead)
/// and handles the drag-to-scrub gesture. `isScrubbing` is exposed via a
/// binding so the parent can react (e.g. blurring the hero artwork while
/// the user drags).
struct PlayerScrubberView: View {

    @Bindable var state: PlaybackState
    @Binding var isScrubbing: Bool
    var chapters: [Episode.Chapter] = []
    var clips: [Clip] = []
    var onClipTap: ((Clip) -> Void)?
    var downloadFraction: Double? = nil

    @State private var scrubTime: TimeInterval = 0
    @State private var timelineWidth: CGFloat = 0

    var body: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            PlayerTimelineView(
                duration: state.duration,
                currentTime: isScrubbing ? scrubTime : state.currentTime,
                isScrubbing: isScrubbing,
                chapters: chapters,
                clips: clips,
                onClipTap: onClipTap,
                downloadFraction: downloadFraction
            )
            .frame(height: 28)
            .background(
                GeometryReader { proxy in
                    Color.clear
                        .onAppear { timelineWidth = proxy.size.width }
                        .onChange(of: proxy.size.width) { _, w in timelineWidth = w }
                }
            )
            .gesture(scrubGesture)
            .accessibilityElement()
            .accessibilityLabel("Playback scrubber")
            .accessibilityValue(PlayerTimeFormat.progress(state.currentTime, state.duration))
            .accessibilityHint("Swipe up or down to skip")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: state.skipForward()
                case .decrement: state.skipBackward()
                @unknown default: break
                }
            }

            HStack {
                Text(PlayerTimeFormat.clock(isScrubbing ? scrubTime : state.currentTime))
                Spacer()
                let elapsed = isScrubbing ? scrubTime : state.currentTime
                let remainingStr = PlayerTimeFormat.remaining(elapsed, duration: state.duration)
                Text(remainingStr.isEmpty ? PlayerTimeFormat.clock(state.duration) : remainingStr)
            }
            .font(AppTheme.Typography.monoCallout)
            .foregroundStyle(.primary)
            .monospacedDigit()
        }
    }

    /// Absolute-position scrub: x = 0 maps to 0 s, x = width maps to
    /// `duration`. `minimumDistance: 4` suppresses the visual scrub-flash
    /// from incidental taps; the previous `0` value triggered expansion
    /// and hero blur on every brush.
    private var scrubGesture: some Gesture {
        DragGesture(minimumDistance: 4)
            .onChanged { value in
                let width = timelineWidth
                guard width > 0 else { return }
                if !isScrubbing {
                    isScrubbing = true
                    scrubTime = state.currentTime
                    Haptics.soft()
                }
                let fraction = max(0, min(1, value.location.x / width))
                scrubTime = state.duration * Double(fraction)
            }
            .onEnded { _ in
                if isScrubbing {
                    state.seekSnapping(to: scrubTime)
                    Haptics.medium()
                }
                isScrubbing = false
            }
    }
}
