import SwiftUI

// MARK: - PlayerTimelineView

/// Scrubber timeline for the full-screen player. Replaces the synthetic
/// waveform with a clean horizontal track that conveys structure and clips:
///
///  • Very faint tick marks at chapter boundaries.
///  • Highlighted ranges for the user's clips on this episode.
///  • Playhead marker at the current position.
///
/// Rendering is done in a `Canvas` for efficiency; clip tap targets are
/// laid out as transparent `Button`s on top so SwiftUI handles hit-testing.
struct PlayerTimelineView: View {

    let duration: TimeInterval
    let currentTime: TimeInterval
    let isScrubbing: Bool
    var chapters: [Episode.Chapter] = []
    var clips: [Clip] = []
    var onClipTap: ((Clip) -> Void)?
    /// Fraction (0–1) of the episode that has been downloaded. When non-nil,
    /// a medium-opacity shade is drawn between the background and playhead fill.
    var downloadFraction: Double? = nil

    // Track geometry constants
    private let trackH: CGFloat = 4
    private let clipH: CGFloat = 10
    private let tickH: CGFloat = 14

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height

            Canvas { ctx, size in
                drawTrack(in: &ctx, size: size)
                drawChapterTicks(in: &ctx, size: size)
                drawClipHighlights(in: &ctx, size: size)
                drawPlayhead(in: &ctx, size: size)
            }
            .accessibilityHidden(true)

            // Invisible tap targets for clip regions
            if let onClipTap, duration > 0 {
                ForEach(clips) { clip in
                    let startFrac = clip.startSeconds / duration
                    let endFrac = clip.endSeconds / duration
                    let startX = startFrac * w
                    let clipWidth = max(20, (endFrac - startFrac) * w + 8)
                    Color.clear
                        .frame(width: clipWidth, height: h)
                        .contentShape(Rectangle())
                        .position(x: startX + clipWidth / 2, y: h / 2)
                        .onTapGesture { onClipTap(clip) }
                }
            }
        }
    }

    // MARK: - Drawing

    private func drawTrack(in ctx: inout GraphicsContext, size: CGSize) {
        let y = (size.height - trackH) / 2
        let rect = CGRect(x: 0, y: y, width: size.width, height: trackH)
        ctx.fill(
            Path(roundedRect: rect, cornerRadius: trackH / 2),
            with: .color(Color.primary.opacity(0.14))
        )
        guard duration > 0 else { return }
        if let dl = downloadFraction, dl > 0 {
            let dlRect = CGRect(x: 0, y: y, width: size.width * min(1, dl), height: trackH)
            ctx.fill(
                Path(roundedRect: dlRect, cornerRadius: trackH / 2),
                with: .color(Color.primary.opacity(0.30))
            )
        }
        let progress = min(1, currentTime / duration)
        let filled = CGRect(x: 0, y: y, width: size.width * progress, height: trackH)
        ctx.fill(
            Path(roundedRect: filled, cornerRadius: trackH / 2),
            with: .color(Color.accentColor)
        )
    }

    private func drawChapterTicks(in ctx: inout GraphicsContext, size: CGSize) {
        guard duration > 0 else { return }
        let midY = size.height / 2
        for chapter in chapters {
            guard chapter.startTime > 0 else { continue }
            let x = (chapter.startTime / duration) * size.width
            let rect = CGRect(
                x: x - 0.5,
                y: midY - tickH / 2,
                width: 1,
                height: tickH
            )
            ctx.fill(Path(rect), with: .color(Color.primary.opacity(0.12)))
        }
    }

    private func drawClipHighlights(in ctx: inout GraphicsContext, size: CGSize) {
        guard duration > 0 else { return }
        let midY = size.height / 2
        for clip in clips {
            let startX = (clip.startSeconds / duration) * size.width
            let endX = (clip.endSeconds / duration) * size.width
            let width = max(2, endX - startX)
            let rect = CGRect(
                x: startX,
                y: midY - clipH / 2,
                width: width,
                height: clipH
            )
            ctx.fill(
                Path(roundedRect: rect, cornerRadius: clipH / 2),
                with: .color(Color.accentColor.opacity(0.45))
            )
        }
    }

    private func drawPlayhead(in ctx: inout GraphicsContext, size: CGSize) {
        guard duration > 0 else { return }
        let x = (currentTime / duration) * size.width
        let midY = size.height / 2
        let radius: CGFloat = 6
        ctx.fill(
            Path(ellipseIn: CGRect(
                x: x - radius,
                y: midY - radius,
                width: radius * 2,
                height: radius * 2
            )),
            with: .color(Color.accentColor)
        )
        // White ring for visibility on light/dark backgrounds
        ctx.stroke(
            Path(ellipseIn: CGRect(
                x: x - radius,
                y: midY - radius,
                width: radius * 2,
                height: radius * 2
            )),
            with: .color(Color.white.opacity(0.85)),
            lineWidth: 1.5
        )
    }
}
