import SwiftUI
import SwiftData
import OSLog

struct LibraryView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Podcast.title) private var podcasts: [Podcast]
    @State private var showingAddPodcast = false
    @State private var showingSettings = false
    var audioService: AudioService
    var processingQueue: ProcessingQueue

    private let podcastService = PodcastService()

    var body: some View {
        NavigationStack {
            List {
                if podcasts.isEmpty {
                    ContentUnavailableView(
                        "No Podcasts",
                        systemImage: "books.vertical",
                        description: Text("Subscribe to podcasts to build your library.")
                    )
                } else {
                    ForEach(podcasts) { podcast in
                        NavigationLink(value: podcast) {
                            PodcastRow(podcast: podcast)
                        }
                    }
                    .onDelete(perform: deletePodcasts)
                }
            }
            .listStyle(.plain)
            .navigationTitle("Library")
            .refreshable {
                await refreshAllFeeds()
            }
            .navigationDestination(for: Podcast.self) { podcast in
                PodcastDetailView(podcast: podcast, audioService: audioService, processingQueue: processingQueue)
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        showingSettings = true
                    } label: {
                        Image(systemName: "gearshape")
                    }
                    .accessibilityIdentifier("settingsButton")
                }

                ToolbarItem(placement: .primaryAction) {
                    Button {
                        showingAddPodcast = true
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityIdentifier("addPodcastButton")
                }
            }
            .sheet(isPresented: $showingAddPodcast) {
                DiscoverView()
            }
            .sheet(isPresented: $showingSettings) {
                SettingsView()
            }
        }
    }

    private func deletePodcasts(at offsets: IndexSet) {
        for index in offsets {
            modelContext.delete(podcasts[index])
        }
    }

    private func refreshAllFeeds() async {
        for podcast in podcasts {
            do {
                _ = try await podcastService.refreshFeed(podcast: podcast)
            } catch {
                Logger.network.error("Failed to refresh feed for \(podcast.title): \(error)")
            }
        }
    }
}

struct PodcastRow: View {
    let podcast: Podcast

    var body: some View {
        HStack(spacing: 12) {
            CachedAsyncImage(url: podcast.artworkURL) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(contentMode: .fill)
            .frame(width: 48, height: 48)
            .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 4) {
                Text(podcast.title)
                    .font(.subheadline)
                    .fontWeight(.medium)

                Text(podcast.author)
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Text("\(podcast.episodes.count) episodes")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }
}

#Preview {
    LibraryView(audioService: AudioService(), processingQueue: ServiceContainer.shared.processingQueue)
        .modelContainer(for: [Podcast.self], inMemory: true)
}
