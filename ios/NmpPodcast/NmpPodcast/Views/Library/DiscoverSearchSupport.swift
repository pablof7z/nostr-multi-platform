import SwiftUI

// MARK: - Supporting Views

struct PodcastSearchRow: View {
    let podcast: PodcastIndexPodcast

    var body: some View {
        HStack(spacing: 12) {
            CachedAsyncImage(url: podcast.artworkURL) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(1, contentMode: .fill)
            .frame(width: 60, height: 60)
            .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 4) {
                Text(podcast.title)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .lineLimit(2)

                if let author = podcast.author {
                    Text(author)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }
}

struct EpisodeSearchRow: View {
    let episode: Episode
    let matchContext: String?

    var body: some View {
        HStack(spacing: 12) {
            CachedAsyncImage(url: episode.podcast?.artworkURL) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(1, contentMode: .fill)
            .frame(width: 60, height: 60)
            .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 4) {
                Text(episode.title)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .lineLimit(2)

                if let podcastTitle = episode.podcast?.title {
                    Text(podcastTitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                if let context = matchContext {
                    Text(context)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(2)
                        .italic()
                }
            }

            Spacer()

            Image(systemName: "play.circle")
                .font(.title3)
                .foregroundStyle(.blue)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }
}
