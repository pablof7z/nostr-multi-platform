import SwiftUI

// MARK: - EpisodeRow

/// Episode list row for the show-detail screen.
///
/// **State surfaces:**
///   - Unplayed:     red `circle.fill` dot badge on artwork, bold title.
///   - In progress:  `circle.lefthalf.filled` "crescent" badge.
///   - Played:       `checkmark.circle.fill` badge, dimmed title.
///   - Downloading:  2 px progress bar (primary color) pinned to bottom edge.
///   - Transcribing: 2 px progress bar (accent color) pinned to bottom edge.
///   - Downloaded:   title at full opacity; not-yet-downloaded titles are muted.
struct EpisodeRow: View {
    let episode: Episode
    let showAccent: Color
    /// Fallback artwork URL when the episode has no per-item `<itunes:image>`.
    /// Typically the parent subscription's image.
    var fallbackImageURL: URL? = nil
    /// When set, renders a small podcast-name caption above the episode title.
    /// Used in cross-show contexts (e.g. Library "All Episodes") where the
    /// artwork alone doesn't make the show obvious.
    var podcastTitle: String? = nil
    /// When provided, a trailing play button is rendered that calls this closure
    /// instead of navigating to the episode detail screen.
    var onPlay: (() -> Void)? = nil

    private static let thumbnailSize: CGFloat = 56

    /// Live progress map — drives the bottom progress bar without hitting
    /// `AppStateStore` on every 5%/200 ms tick.
    @State private var downloadService = EpisodeDownloadService.shared

    var body: some View {
        HStack(alignment: .center, spacing: AppTheme.Spacing.md) {
            thumbnail
                .overlay(alignment: .topLeading) { stateBadge }

            VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
                if let podcastTitle {
                    Text(podcastTitle)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
                Text(episode.title)
                    .font(AppTheme.Typography.headline)
                    .foregroundStyle(titleColor)
                    .lineLimit(2)

                let summary = episode.plainTextSummary
                if !summary.isEmpty {
                    Text(summary)
                        .font(AppTheme.Typography.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                metaRow
            }

            if let onPlay {
                Spacer()
                Button {
                    Haptics.medium()
                    onPlay()
                } label: {
                    Image(systemName: "play.circle.fill")
                        .font(.title2)
                        .foregroundStyle(showAccent)
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Play \(episode.title)")
            }
        }
        .padding(.vertical, AppTheme.Spacing.sm)
        .overlay(alignment: .bottom) { downloadProgressBar }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    // MARK: - Thumbnail

    private var artworkURL: URL? { episode.imageURL ?? fallbackImageURL }

    @ViewBuilder
    private var thumbnail: some View {
        let shape = RoundedRectangle(cornerRadius: AppTheme.Corner.md, style: .continuous)
        Group {
            if let url = artworkURL {
                CachedAsyncImage(
                    url: url,
                    targetSize: CGSize(width: Self.thumbnailSize, height: Self.thumbnailSize)
                ) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        thumbnailPlaceholder
                    }
                }
            } else {
                thumbnailPlaceholder
            }
        }
        .frame(width: Self.thumbnailSize, height: Self.thumbnailSize)
        .clipShape(shape)
        .accessibilityHidden(true)
    }

    private var thumbnailPlaceholder: some View {
        ZStack {
            Color.secondary.opacity(0.18)
            Image(systemName: "waveform")
                .font(.system(size: 20, weight: .light))
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Subviews

    /// Decorative state badge in the top-leading corner of the thumbnail.
    private var stateBadge: some View {
        stateIndicator.padding(6)
    }

    @ViewBuilder
    private var stateIndicator: some View {
        if episode.played {
            badgeChip {
                Image(systemName: "checkmark")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(.white)
            }
        } else if episode.isInProgress {
            badgeChip {
                Image(systemName: "circle.lefthalf.filled")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(showAccent)
            }
        } else {
            Circle()
                .fill(AppTheme.Tint.error)
                .frame(width: 10, height: 10)
                .overlay(Circle().stroke(Color.white, lineWidth: 1.5))
        }
    }

    private func badgeChip<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        ZStack {
            Circle().fill(.ultraThinMaterial)
            Circle().fill(Color.black.opacity(0.35))
            content()
        }
        .frame(width: 16, height: 16)
    }

    private var metaRow: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Text(episode.formattedDuration)
                .font(AppTheme.Typography.monoCaption)
                .foregroundStyle(.secondary)

            Text("·")
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.tertiary)

            Text(relativePublished)
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)

            if episode.isInProgress {
                Text("·")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.tertiary)
                Text("\(Int((episode.playbackProgress * 100).rounded()))% in")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(showAccent)
            }
        }
    }

    // MARK: - Helpers

    /// Mutes the title for not-yet-downloaded episodes so the user can
    /// distinguish locally available episodes at a glance. Played episodes
    /// stay secondary regardless of download state.
    private var titleColor: Color {
        if episode.played { return .secondary }
        if case .downloaded = episode.downloadState { return .primary }
        return Color.primary.opacity(0.55)
    }

    @ViewBuilder
    private var downloadProgressBar: some View {
        if case .downloading(let persisted, _) = episode.downloadState {
            let p = (downloadService.progress[episode.id] ?? persisted).clamped01
            thinProgressBar(progress: p, color: Color.primary)
        } else if case .downloaded = episode.downloadState,
                  case .transcribing(let p) = episode.transcriptState {
            thinProgressBar(progress: p.clamped01, color: Color.accentColor)
        }
    }

    private func thinProgressBar(progress: Double, color: Color) -> some View {
        GeometryReader { geo in
            Rectangle()
                .fill(color)
                .frame(width: geo.size.width * progress, height: 2)
        }
        .frame(height: 2)
    }

    private var relativePublished: String {
        Self.relativeFormatter.localizedString(for: episode.pubDate, relativeTo: Date())
    }

    /// Cached — `EpisodeRow` is the per-row view inside a show's
    /// episode list, so a 200-episode show was minting 200
    /// `RelativeDateTimeFormatter` instances per render.
    /// `RelativeDateTimeFormatter` is reentrant for `localizedString(...)`.
    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .abbreviated
        return f
    }()

    private var accessibilityLabel: String {
        var parts: [String] = [episode.title]
        parts.append(episode.formattedDuration)
        if episode.played {
            parts.append("played")
        } else if episode.isInProgress {
            parts.append("\(Int((episode.playbackProgress * 100).rounded())) percent listened")
        } else {
            parts.append("unplayed")
        }
        switch episode.downloadState {
        case .downloading(let persisted, _):
            let pct = Int(((downloadService.progress[episode.id] ?? persisted).clamped01 * 100).rounded())
            parts.append("downloading \(pct) percent")
        case .downloaded:
            switch episode.transcriptState {
            case .transcribing(let p):
                parts.append("transcribing \(Int((p.clamped01 * 100).rounded())) percent")
            case .queued, .fetchingPublisher:
                parts.append("transcript queued")
            case .ready:
                parts.append("transcript available")
            case .failed:
                parts.append("transcript failed")
            case .none:
                parts.append("downloaded")
            }
        case .failed:
            parts.append("download failed")
        case .queued:
            parts.append("download queued")
        case .notDownloaded:
            break
        }
        return parts.joined(separator: ", ")
    }
}
