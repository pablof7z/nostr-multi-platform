import SwiftUI

// MARK: - Topic Search View

struct TopicSearchView: View {
    let topic: String
    @State private var podcasts: [PodcastIndexPodcast] = []
    @State private var isLoading = false
    @State private var selectedPodcast: PodcastIndexPodcast?

    private let podcastIndexService = ServiceContainer.shared.podcastIndexService

    var body: some View {
        ScrollView {
            if isLoading {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .padding(.top, 40)
            } else if podcasts.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "magnifyingglass")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text("No podcasts found for \"\(topic)\"")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 40)
            } else {
                LazyVStack(spacing: 0) {
                    ForEach(podcasts) { podcast in
                        PodcastSearchRow(podcast: podcast)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                selectedPodcast = podcast
                            }
                        Divider()
                            .padding(.leading, 84)
                    }
                }
            }
        }
        .navigationTitle(topic)
        .navigationBarTitleDisplayMode(.large)
        .sheet(item: $selectedPodcast) { podcast in
            PodcastDetailSheet(podcast: podcast)
        }
        .task {
            await searchTopic()
        }
    }

    private func searchTopic() async {
        isLoading = true
        do {
            let results = try await podcastIndexService.search(term: topic, limit: 30)
            await MainActor.run {
                podcasts = results
                isLoading = false
            }
        } catch {
            await MainActor.run {
                isLoading = false
            }
        }
    }
}
