import SwiftUI
import SwiftData

struct FeedView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Episode.publishedAt, order: .reverse) private var episodes: [Episode]
    var audioService: AudioService
    var processingQueue: ProcessingQueue

    var body: some View {
        NavigationStack {
            List {
                if episodes.isEmpty {
                    ContentUnavailableView(
                        "No Episodes",
                        systemImage: "waveform",
                        description: Text("Subscribe to podcasts to see new episodes here.")
                    )
                } else {
                    ForEach(episodes) { episode in
                        EpisodeRow(episode: episode, audioService: audioService, processingQueue: processingQueue)
                            .swipeActions(edge: .leading) {
                                if episode.downloadState != .downloaded {
                                    Button {
                                        processingQueue.enqueuePriority(episode: episode)
                                    } label: {
                                        Label("Prioritize", systemImage: "arrow.up.circle.fill")
                                    }
                                    .tint(.blue)
                                }
                            }
                    }
                    .onDelete(perform: deleteEpisodes)
                }
            }
            .listStyle(.plain)
            .navigationTitle("Feed")
        }
    }

    private func deleteEpisodes(at offsets: IndexSet) {
        for index in offsets {
            modelContext.delete(episodes[index])
        }
    }
}

#Preview {
    FeedView(audioService: AudioService(), processingQueue: ServiceContainer.shared.processingQueue)
        .modelContainer(for: [Podcast.self, Episode.self], inMemory: true)
}
