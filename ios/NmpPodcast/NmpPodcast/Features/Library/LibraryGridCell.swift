import SwiftUI

// MARK: - LibraryGridCell

/// One card in the Library subscriptions grid.
///
/// **Glass usage:** matte. The cell uses a rounded artwork tile, a title line,
/// and an unplayed dot — no glass. The grid container itself is the only glass-
/// allowed surface, and the brief reserves it for the chrome (filter rail,
/// search bar).
///
/// Artwork is loaded asynchronously from `podcast.imageURL`; while the
/// image is in-flight (or absent) we render a tinted SF Symbol stand-in keyed
/// to `podcast.accentColor`.
struct LibraryGridCell: View {
    let podcast: Podcast
    let unplayedCount: Int
    let category: PodcastCategory?

    init(
        podcast: Podcast,
        unplayedCount: Int,
        category: PodcastCategory? = nil
    ) {
        self.podcast = podcast
        self.unplayedCount = unplayedCount
        self.category = category
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            artworkTile

            VStack(alignment: .leading, spacing: 2) {
                Text(podcast.title)
                    .font(AppTheme.Typography.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)

                if !podcast.author.isEmpty {
                    Text(podcast.author)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                if let category {
                    categoryBadge(category)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    // MARK: - Pieces

    private var artworkTile: some View {
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
            .aspectRatio(1, contentMode: .fit)
            .overlay(artworkOverlay)
            .clipShape(RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous))
            .appShadow(AppTheme.Shadow.subtle)
    }

    @ViewBuilder
    private var artworkOverlay: some View {
        if let url = podcast.imageURL {
            CachedAsyncImage(url: url, targetSize: CGSize(width: 150, height: 150)) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                default:
                    symbolPlaceholder
                }
            }
        } else {
            symbolPlaceholder
        }
    }

    private var symbolPlaceholder: some View {
        Image(systemName: podcast.artworkSymbol)
            .font(.system(size: 44, weight: .light))
            .foregroundStyle(.white.opacity(0.92))
            .accessibilityHidden(true)
    }

    private func categoryBadge(_ category: PodcastCategory) -> some View {
        Text(category.name)
            .font(.caption2.weight(.semibold))
            .lineLimit(1)
            .foregroundStyle(.secondary)
            .padding(.horizontal, AppTheme.Spacing.xs)
            .padding(.vertical, 2)
            .background(Color(.tertiarySystemFill), in: Capsule(style: .continuous))
            .padding(.top, 2)
    }

    private var accessibilityLabel: String {
        var parts = [podcast.title]
        if !podcast.author.isEmpty { parts.append(podcast.author) }
        if let category { parts.append(category.name) }
        if unplayedCount > 0 { parts.append("\(unplayedCount) unplayed") }
        return parts.joined(separator: ", ")
    }
}
