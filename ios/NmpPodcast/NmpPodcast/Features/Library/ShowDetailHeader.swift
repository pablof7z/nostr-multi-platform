import SwiftUI

// MARK: - ShowDetailHeader

/// Hero header for `ShowDetailView` — square artwork on the leading edge with
/// title, author, description (3-line cap), and meta row stacked to the right.
///
/// **Tint:** the screen-level gradient lives in `ShowDetailView` so it can
/// bleed past the safe area / nav bar; the header itself is matte and sits
/// on top of that gradient.
///
/// **Glass:** none. The header is a matte editorial surface.
struct ShowDetailHeader: View {
    let podcast: Podcast
    let episodeCount: Int

    private static let artworkSize: CGFloat = 116

    var body: some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.md) {
            artwork

            VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
                Text(podcast.title)
                    .font(AppTheme.Typography.title)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)

                if !podcast.author.isEmpty {
                    Text(podcast.author)
                        .font(AppTheme.Typography.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                let body = EpisodeShowNotesFormatter.plainText(from: podcast.description)
                if !body.isEmpty {
                    Text(body)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, AppTheme.Spacing.xs)
                }

                metaRow
                    .padding(.top, AppTheme.Spacing.xs)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
        .padding(.top, AppTheme.Spacing.lg)
        .padding(.bottom, AppTheme.Spacing.md)
    }

    // MARK: - Pieces

    private var artwork: some View {
        RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
            .fill(
                LinearGradient(
                    colors: [
                        podcast.accentColor.opacity(0.95),
                        podcast.accentColor.opacity(0.55)
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
            .overlay(artworkOverlay)
            .frame(width: Self.artworkSize, height: Self.artworkSize)
            .clipShape(RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous))
            .appShadow(AppTheme.Shadow.lifted)
    }

    @ViewBuilder
    private var artworkOverlay: some View {
        if let url = podcast.imageURL {
            CachedAsyncImage(url: url) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                default:
                    artworkSymbol
                }
            }
        } else {
            artworkSymbol
        }
    }

    private var artworkSymbol: some View {
        Image(systemName: podcast.artworkSymbol)
            .font(.system(size: 44, weight: .light))
            .foregroundStyle(.white.opacity(0.92))
            .accessibilityHidden(true)
    }

    private var metaRow: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Text("\(episodeCount) \(episodeCount == 1 ? "episode" : "episodes")")
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)
            if let refreshed = podcast.lastRefreshedAt {
                Text("·")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.tertiary)
                Text("Updated \(relative(refreshed))")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }

    private func relative(_ date: Date) -> String {
        Self.relativeFormatter.localizedString(for: date, relativeTo: Date())
    }

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .abbreviated
        return f
    }()
}
