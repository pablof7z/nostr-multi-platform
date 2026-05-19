import SwiftUI

// MARK: - DownloadProgressBadge

/// Ambient, read-only badge surfacing an episode's download lifecycle on the
/// Now Playing and mini-player surfaces. Visually quiet — never a CTA.
///
/// Lifecycle mapping (on `Episode.downloadState`):
///   - `.notDownloaded`  → hidden (no surface).
///   - `.queued`         → `clock` glyph, no text.
///   - `.downloading`    → `arrow.down.circle` + percentage.
///   - `.downloaded`     → `checkmark.circle.fill` glyph, no text.
///   - `.failed`         → `exclamationmark.triangle.fill` in error tint.
///
/// `liveProgress` overrides the persisted `.downloading(progress, _)` value
/// so the percentage updates smoothly with `EpisodeDownloadService.progress`
/// (5%/200ms) without each tick going through `AppStateStore`. Mirrors the
/// pattern used by `EpisodeRow` + `DownloadStatusCapsule`.
///
/// **Glass usage:** plain `.regular` glass in a capsule. State distinction
/// is encoded in `foregroundStyle` (e.g. `.failed` reads red) rather than
/// by tinting the material — keep the surface neutral so the badge fades
/// into the player chrome instead of stealing focus.
struct DownloadProgressBadge: View {
    let episode: Episode
    /// Live progress in `0...1` from `EpisodeDownloadService.progress[id]`.
    /// `nil` falls back to the value baked into `episode.downloadState`.
    var liveProgress: Double? = nil

    var body: some View {
        if let render = render {
            HStack(spacing: 4) {
                Image(systemName: render.symbol)
                    .font(.caption2.weight(.semibold))
                if let label = render.label {
                    Text(label)
                        .font(AppTheme.Typography.caption)
                        .monospacedDigit()
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, render.label == nil ? 6 : AppTheme.Spacing.sm)
            .padding(.vertical, 4)
            .foregroundStyle(render.foreground)
            .glassEffect(.regular, in: .capsule)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(render.accessibilityLabel)
        }
    }

    // MARK: - Render

    private struct Render {
        let symbol: String
        /// `nil` for icon-only states (`.downloaded`, `.queued`, `.failed`).
        let label: String?
        let foreground: AnyShapeStyle
        let accessibilityLabel: String
    }

    private var render: Render? {
        switch episode.downloadState {
        case .notDownloaded:
            return nil
        case .queued:
            return Render(
                symbol: "clock",
                label: nil,
                foreground: AnyShapeStyle(.secondary),
                accessibilityLabel: "Download queued"
            )
        case .downloading(let persisted, _):
            let resolved = (liveProgress ?? persisted).clamped01
            let pct = Int((resolved * 100).rounded())
            return Render(
                symbol: "arrow.down.circle",
                label: "\(pct)%",
                foreground: AnyShapeStyle(.primary),
                accessibilityLabel: "Downloading, \(pct) percent"
            )
        case .downloaded:
            return Render(
                symbol: "checkmark.circle.fill",
                label: nil,
                foreground: AnyShapeStyle(.secondary),
                accessibilityLabel: "Downloaded"
            )
        case .failed:
            return Render(
                symbol: "exclamationmark.triangle.fill",
                label: nil,
                foreground: AnyShapeStyle(AppTheme.Tint.error),
                accessibilityLabel: "Download failed"
            )
        }
    }
}

// `Double.clamped01` lives in `Design/NumberExtensions.swift`.
