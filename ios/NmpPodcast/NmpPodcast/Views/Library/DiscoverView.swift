import SwiftUI
import SwiftData

struct DiscoverView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    @Query private var subscribedPodcasts: [Podcast]
    @Query(sort: \Episode.publishedAt, order: .reverse) private var allEpisodes: [Episode]

    @State private var searchText = ""
    @State private var trendingPodcasts: [PodcastIndexPodcast] = []
    @State private var searchResults: [PodcastIndexPodcast] = []
    @State private var episodeSearchResults: [EpisodeSearchResult] = []
    @State private var recommendations: [PodcastRecommendation] = []
    @State private var personalizedHero: PersonalizedHero?
    @State private var userTopics: [String] = []

    @State private var isLoadingTrending = false
    @State private var isLoadingRecommendations = false
    @State private var isSearching = false
    @State private var isSearchingEpisodes = false
    @State private var errorMessage: String?

    @State private var showAddByURL = false
    @State private var selectedPodcast: PodcastIndexPodcast?
    @State private var selectedEpisode: Episode?
    @State private var showAllTrending = false
    @State private var showAllCategories = false

    private let podcastIndexService = ServiceContainer.shared.podcastIndexService
    private let recommendationService = ServiceContainer.shared.recommendationService

    struct EpisodeSearchResult: Identifiable {
        let id: UUID
        let episode: Episode
        let matchContext: String?
    }

    private var listenedEpisodes: [Episode] {
        allEpisodes.filter { $0.hasBeenPlayed || $0.playbackPosition > 0 }
    }

    private var hasPersonalization: Bool {
        !subscribedPodcasts.isEmpty && !listenedEpisodes.isEmpty
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 28) {
                    if !searchText.isEmpty {
                        searchResultsSection
                    } else {
                        // Personalized Hero or Trending Hero
                        heroSection

                        // For You (if personalized)
                        if hasPersonalization && !recommendations.isEmpty {
                            forYouSection
                        }

                        // Trending
                        trendingSection

                        // Categories
                        categoriesSection

                        // Topics (if personalized)
                        if hasPersonalization && !userTopics.isEmpty {
                            topicsSection
                        }

                        // Add by URL
                        addByURLSection
                    }
                }
                .padding(.vertical)
            }
            .searchable(text: $searchText, prompt: "Search podcasts & episodes")
            .onChange(of: searchText) { _, newValue in
                if !newValue.isEmpty {
                    performSearch()
                } else {
                    searchResults = []
                    isSearching = false
                }
            }
            .navigationTitle("Discover")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
            }
            .sheet(item: $selectedPodcast) { podcast in
                PodcastDetailSheet(podcast: podcast)
            }
            .sheet(item: $selectedEpisode) { episode in
                NavigationStack {
                    EpisodeDetailView(
                        episode: episode,
                        audioService: ServiceContainer.shared.audioService
                    )
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Close") {
                                selectedEpisode = nil
                            }
                        }
                    }
                }
            }
            .sheet(isPresented: $showAddByURL) {
                AddPodcastView()
            }
            .sheet(isPresented: $showAllTrending) {
                AllTrendingView(podcasts: trendingPodcasts)
            }
            .sheet(isPresented: $showAllCategories) {
                AllCategoriesView()
            }
            .task {
                await loadContent()
            }
        }
    }
}

#Preview {
    DiscoverView()
        .modelContainer(for: [Podcast.self, Episode.self], inMemory: true)
}
