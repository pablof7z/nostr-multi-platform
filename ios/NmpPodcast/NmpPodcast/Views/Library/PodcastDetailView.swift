import SwiftUI
import SwiftData
import OSLog

struct PodcastDetailView: View {
    let podcast: Podcast
    var audioService: AudioService
    var processingQueue: ProcessingQueue

    private let podcastService = PodcastService()

    private var sortedEpisodes: [Episode] {
        podcast.episodes.sorted { $0.publishedAt > $1.publishedAt }
    }

    var body: some View {
        List {
            ForEach(sortedEpisodes) { episode in
                EpisodeRow(episode: episode, audioService: audioService, processingQueue: processingQueue)
            }
        }
        .listStyle(.plain)
        .navigationTitle(podcast.title)
        .navigationBarTitleDisplayMode(.large)
        .refreshable {
            do {
                _ = try await podcastService.refreshFeed(podcast: podcast)
            } catch {
                Logger.network.error("Failed to refresh feed for \(podcast.title): \(error)")
            }
        }
    }
}

#Preview {
    let podcast = Podcast(
        feedURL: URL(string: "https://example.com/feed")!,
        title: "Tim Ferriss",
        author: "Tim Ferriss"
    )

    return NavigationStack {
        PodcastDetailView(podcast: podcast, audioService: AudioService(), processingQueue: ServiceContainer.shared.processingQueue)
    }
}
