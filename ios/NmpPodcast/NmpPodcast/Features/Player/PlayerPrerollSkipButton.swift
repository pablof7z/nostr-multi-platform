import SwiftUI

// MARK: - PlayerPrerollSkipButton
//
// Transient "Skip 30s ad" button anchored above the scrubber when the
// currently-loaded episode has a pre-roll ad and the playhead is still
// inside it. Auto-hides when the playhead moves past the segment's `end`.
//
// Tapping calls `state.seek(to:)` with `segment.end` directly — that goes
// through the normal seek + flush path (haptic, persist, widget snapshot)
// so the user gets the same feedback as any other manual scrub.

struct PlayerPrerollSkipButton: View {
    @Bindable var state: PlaybackState
    let episode: Episode?

    var body: some View {
        if let segment = activePrerollSegment {
            Button {
                Haptics.medium()
                state.seek(to: segment.end)
            } label: {
                let remaining = max(0, Int(segment.end - state.currentTime))
                Label(
                    remaining > 0 ? "Skip \(remaining)s ad" : "Skip ad",
                    systemImage: "forward.end.fill"
                )
                .font(.system(.subheadline, design: .rounded).weight(.semibold))
                .foregroundStyle(.primary)
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .glassEffect(.regular.interactive(), in: .capsule)
            }
            .buttonStyle(.plain)
            .transition(.asymmetric(
                insertion: .opacity.combined(with: .move(edge: .bottom)),
                removal: .opacity
            ))
            .accessibilityLabel("Skip pre-roll ad")
        }
    }

    /// First pre-roll segment whose `[start, end)` still contains the
    /// playhead. Nil when there is no pre-roll, or when the playhead has
    /// already moved past it. We don't surface mid- or post-roll buttons
    /// here — the auto-skip toggle is the right control for those.
    private var activePrerollSegment: Episode.AdSegment? {
        guard let segments = episode?.adSegments, !segments.isEmpty else { return nil }
        let t = state.currentTime
        return segments.first { ad in
            ad.kind == .preroll && t >= ad.start && t < ad.end
        }
    }
}
