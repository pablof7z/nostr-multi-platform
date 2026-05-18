import SwiftUI

// MARK: - PlayerCompactTitleView
//
// Compact artwork + episode title that occupies the middle slot of the
// player top bar once the user has scrolled the hero header offscreen.
// Native Music / Podcasts pattern: a tiny rounded thumbnail (~28 pt)
// sits next to a single-line, truncated episode title.
//
// Sized to fit the gap between the close button on the leading edge and
// the share / AirPlay / more cluster on the trailing edge — see
// `PlayerView.topBar`. Show name still appears as a kicker above the
// episode title so the user keeps context.

struct PlayerCompactTitleView: View {
    let artworkURL: URL?
    let episodeTitle: String
    let showName: String

    var body: some View {
        HStack(spacing: AppTheme.Spacing.xs) {
            thumbnail
            VStack(alignment: .leading, spacing: 0) {
                if !showName.isEmpty {
                    Text(showName)
                        .font(.system(size: 10, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Text(episodeTitle)
                    .font(AppTheme.Typography.caption.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(showName.isEmpty ? episodeTitle : "\(showName), \(episodeTitle)")
    }

    @ViewBuilder
    private var thumbnail: some View {
        let size: CGFloat = 28
        ZStack {
            if let url = artworkURL {
                CachedAsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        fallback
                    }
                }
                .id(url)
            } else {
                fallback
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        .accessibilityHidden(true)
    }

    private var fallback: some View {
        ZStack {
            Color.secondary.opacity(0.10)
            Image(systemName: "waveform")
                .font(.system(size: 12, weight: .light))
                .foregroundStyle(.secondary)
        }
    }
}
