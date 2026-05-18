import SwiftUI

/// Primary transport row — speed / skip-back / play-pause / skip-forward / mic / snip.
///
/// All controls sit in a single `HStack` so the row stays at one vertical
/// height while the play/pause circle dominates the center. Speed chip flanks
/// the transport on the left; voice-note mic and auto-snip bookmark flank on
/// the right.
///
/// Long-press on either skip button jumps to the next/previous chapter when
/// the episode exposes navigable ones — same gesture iOS Music uses for
/// previous/next track. Tap remains the configured-seconds skip.
struct PlayerControlsView: View {

    @Bindable var state: PlaybackState
    let glassNamespace: Namespace.ID
    var chapters: [Episode.Chapter] = []
    @Binding var showVoiceNoteSheet: Bool

    var body: some View {
        HStack(alignment: .center, spacing: AppTheme.Spacing.sm) {
            // Right actions: voice note + auto-snip (left side of transport)
            actionChip(glyph: "mic", accessibilityName: "Voice note") {
                Haptics.selection()
                showVoiceNoteSheet = true
            }

            Spacer(minLength: 0)

            // Transport: skip-back / play-pause / skip-forward
            let back = state.skipBackwardSeconds
            let forward = state.skipForwardSeconds
            SkipButton(
                seconds: back,
                direction: .backward,
                tapAction: { state.skipBackward() },
                chapterAction: chapters.isEmpty ? nil : { state.seekToPreviousChapter(in: chapters) }
            )

            playPauseButton

            SkipButton(
                seconds: forward,
                direction: .forward,
                tapAction: { state.skipForward() },
                chapterAction: chapters.isEmpty ? nil : { state.seekToNextChapter(in: chapters) }
            )

            Spacer(minLength: 0)

            AutoSnipButton()
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Subviews

    private var playPauseButton: some View {
        Button {
            state.togglePlayPause()
        } label: {
            Image(systemName: state.isPlaying ? "pause.fill" : "play.fill")
                .font(.largeTitle.weight(.bold))
                .foregroundStyle(.primary)
                .frame(width: 76, height: 76)
                .contentShape(Circle())
                .glassEffect(.regular.interactive(), in: .circle)
                .glassEffectID("player.play", in: glassNamespace)
                .accessibilityLabel(state.isPlaying ? "Pause" : "Play")
        }
        .buttonStyle(.pressable(scale: 0.94, opacity: 0.9))
    }

    private func actionChip(
        glyph: String,
        accessibilityName: String,
        accessibilityValue: String? = nil,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: glyph)
                .font(.title3.weight(.semibold))
                .foregroundStyle(.primary)
                .frame(width: 44, height: 44)
                .contentShape(Circle())
                .glassEffect(.regular.interactive(), in: .circle)
        }
        .buttonStyle(.pressable)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilityName)
        .accessibilityValue(accessibilityValue ?? "")
        .accessibilityAddTraits(.isButton)
    }
}

// MARK: - SkipButton

/// Tap = configured-seconds skip; long-press = chapter nav (when chapters
/// available). `simultaneousGesture` previously fired BOTH actions because
/// it doesn't suppress the Button's tap on release — this struct guards
/// the tap with a `didLongPress` flag so a long-press is exclusive.
///
/// The SF Symbol picker requires an exact match so the visible number
/// always equals the configured interval. A user with a 20 s skip used to
/// see `goforward.15` (closest stocked variant ±5 s) but the action
/// skipped 20 s — the visible label silently lied. Anything off-grid now
/// falls back to bare `goforward`/`gobackward` (no digit) so the visual
/// stays honest.
private struct SkipButton: View {

    enum Direction { case forward, backward }

    let seconds: Int
    let direction: Direction
    let tapAction: () -> Void
    let chapterAction: (() -> Void)?

    @State private var didLongPress = false

    var body: some View {
        let label = Image(systemName: glyph)
            .font(.title3.weight(.semibold))
            .foregroundStyle(.primary)
            .frame(width: 56, height: 56)
            .contentShape(Circle())
            .glassEffect(.regular.interactive(), in: .circle)

        let baseLabel: String = direction == .backward
            ? "Skip back \(seconds) seconds"
            : "Skip forward \(seconds) seconds"

        Button {
            if didLongPress {
                didLongPress = false
                return
            }
            tapAction()
        } label: { label }
        .buttonStyle(.pressable)
        .modifier(LongPressChapterModifier(
            chapterAction: chapterAction,
            didLongPress: $didLongPress
        ))
        .accessibilityLabel(baseLabel)
        .modifier(ChapterAccessibilityActionModifier(
            direction: direction,
            chapterAction: chapterAction
        ))
    }

    private var glyph: String {
        let supported = [10, 15, 30, 45, 60, 75, 90]
        let prefix = direction == .forward ? "goforward" : "gobackward"
        // Exact match only — see comment above on why.
        guard supported.contains(seconds) else { return prefix }
        return "\(prefix).\(seconds)"
    }
}

private struct LongPressChapterModifier: ViewModifier {
    let chapterAction: (() -> Void)?
    @Binding var didLongPress: Bool

    func body(content: Content) -> some View {
        if let chapterAction {
            content.simultaneousGesture(
                LongPressGesture(minimumDuration: 0.45)
                    .onEnded { _ in
                        didLongPress = true
                        Haptics.heavy()
                        chapterAction()
                    }
            )
        } else {
            content
        }
    }
}

private struct ChapterAccessibilityActionModifier: ViewModifier {
    let direction: SkipButton.Direction
    let chapterAction: (() -> Void)?

    func body(content: Content) -> some View {
        if let chapterAction {
            content.accessibilityAction(named: direction == .backward ? "Previous chapter" : "Next chapter") {
                chapterAction()
            }
        } else {
            content
        }
    }
}
