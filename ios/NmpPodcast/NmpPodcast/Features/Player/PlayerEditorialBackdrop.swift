import SwiftUI

// MARK: - PlayerEditorialBackdrop
//
// Cinematic full-bleed backdrop for the Now-Playing surface. Takes the same
// `artworkURL` the hero uses, scales it up, heavy-blurs it, and washes it out
// with a vertical gradient so foreground text + Liquid Glass controls always
// have the contrast they need. This is what gives Castro / Apple Music their
// signature "the cover IS the room" identity — Liquid Glass leapfrogs them
// because the blurred art layers correctly with `.glassEffect()` on top.
//
// The backdrop is its own file so `PlayerView` (314 lines, near the soft cap)
// stays small and the backdrop can be unit-tested or swapped per-style later
// (e.g. an "Editorial Off" preference, or a per-show override that uses a
// solid color instead of the cover).
//
// Implementation notes:
//
//   - We do NOT extract dominant colors with k-means or CIAreaAverage. Scale-
//     and-blur reads as "this episode's color" without the upfront work and
//     handles transitions between chapters with topic-aligned imagery for free
//     (the hero's `artworkURL` already swaps mid-playback for chapter art).
//   - `blur(radius:opaque:)` with `opaque: true` keeps the GPU on the
//     fast-path (no transparent edge sampling) — the scaled artwork already
//     covers all edges, so the opaque flag is safe.
//   - Saturation is bumped slightly to compensate for the wash-out gradient
//     stealing chroma from the eye.
//   - Animation is keyed on `artworkURL` so a chapter-image swap produces a
//     soft cross-fade in lockstep with the hero artwork transition.
struct PlayerEditorialBackdrop: View {

    let artworkURL: URL?

    var body: some View {
        ZStack {
            // Always-present base. Guarantees the backdrop never reads as
            // pure black before the artwork resolves and matches whatever
            // the user's appearance setting prefers underneath.
            Color(uiColor: .systemBackground)

            if let url = artworkURL {
                CachedAsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .scaledToFill()
                            .saturation(1.35)
                            .blur(radius: 80, opaque: true)
                            .opacity(0.65)
                    default:
                        Color.clear
                    }
                }
                .id(url)
                .transition(.opacity)
            }

            // Vertical wash. Top stays artwork-forward (we want to see the
            // color identity behind the editorial header). Bottom darkens
            // toward the system background so the transport controls never
            // sit on a hot saturated band that fights their glass material.
            LinearGradient(
                colors: [
                    Color(uiColor: .systemBackground).opacity(0.10),
                    Color(uiColor: .systemBackground).opacity(0.55),
                    Color(uiColor: .systemBackground).opacity(0.85)
                ],
                startPoint: .top,
                endPoint: .bottom
            )
        }
        .ignoresSafeArea()
        .animation(.easeInOut(duration: 0.6), value: artworkURL)
        .accessibilityHidden(true)
    }
}
