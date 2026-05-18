import SwiftUI

extension DiscoverView {
    // MARK: - Data Loading

    func loadContent() async {
        async let trendingTask: () = loadTrending()
        async let recommendationsTask: () = loadRecommendations()
        async let topicsTask: () = loadUserTopics()

        _ = await (trendingTask, recommendationsTask, topicsTask)
    }

    func loadTrending() async {
        isLoadingTrending = true
        do {
            let podcasts = try await podcastIndexService.trending(limit: 20)
            await MainActor.run {
                trendingPodcasts = podcasts
                isLoadingTrending = false
            }
        } catch {
            await MainActor.run {
                errorMessage = error.localizedDescription
                isLoadingTrending = false
            }
        }
    }

    func loadRecommendations() async {
        guard hasPersonalization else { return }

        isLoadingRecommendations = true

        // Extract data on main actor before passing to service
        let podcasts = subscribedPodcasts
        let episodes = listenedEpisodes

        do {
            let recs = try await recommendationService.getRecommendations(
                subscribedPodcastIDs: podcasts.map { $0.id.uuidString },
                episodeData: episodes.map { RecommendationService.EpisodeData(
                    title: $0.title,
                    summary: $0.aiSummary,
                    description: $0.descriptionText,
                    hasTranscript: $0.transcript != nil
                )},
                limit: 10
            )

            let hero = try await recommendationService.getPersonalizedHero(
                subscribedPodcastIDs: podcasts.map { $0.id.uuidString },
                episodeData: episodes.map { RecommendationService.EpisodeData(
                    title: $0.title,
                    summary: $0.aiSummary,
                    description: $0.descriptionText,
                    hasTranscript: $0.transcript != nil
                )}
            )

            await MainActor.run {
                recommendations = recs
                personalizedHero = hero
                isLoadingRecommendations = false
            }
        } catch {
            await MainActor.run {
                isLoadingRecommendations = false
            }
        }
    }

    func loadUserTopics() async {
        guard hasPersonalization else { return }

        // Extract data on main actor
        let episodes = listenedEpisodes
        let episodeData = episodes.map { RecommendationService.EpisodeData(
            title: $0.title,
            summary: $0.aiSummary,
            description: $0.descriptionText,
            hasTranscript: $0.transcript != nil
        )}

        let topics = await recommendationService.getUserTopics(from: episodeData)
        await MainActor.run {
            userTopics = topics
        }
    }

    func performSearch() {
        guard !searchText.isEmpty else {
            searchResults = []
            episodeSearchResults = []
            isSearching = false
            isSearchingEpisodes = false
            return
        }

        // Search podcasts from PodcastIndex
        isSearching = true
        Task {
            do {
                let results = try await podcastIndexService.search(term: searchText, limit: 20)
                await MainActor.run {
                    searchResults = results
                    isSearching = false
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isSearching = false
                }
            }
        }

        // Search local episodes (title, summary, transcript)
        isSearchingEpisodes = true
        let query = searchText.lowercased()
        let localResults = allEpisodes.filter { episode in
            episode.title.lowercased().contains(query) ||
            (episode.aiSummary?.lowercased().contains(query) ?? false) ||
            (episode.descriptionText?.lowercased().contains(query) ?? false) ||
            (episode.transcript?.fullText.lowercased().contains(query) ?? false)
        }.prefix(10).map { episode in
            let context: String?
            if episode.title.lowercased().contains(query) {
                context = nil
            } else if let summary = episode.aiSummary, summary.lowercased().contains(query) {
                context = extractContext(from: summary, query: query)
            } else if let transcript = episode.transcript?.fullText, transcript.lowercased().contains(query) {
                context = extractContext(from: transcript, query: query)
            } else {
                context = nil
            }
            return EpisodeSearchResult(id: episode.id, episode: episode, matchContext: context)
        }

        episodeSearchResults = Array(localResults)
        isSearchingEpisodes = false
    }

    func extractContext(from text: String, query: String) -> String? {
        guard let range = text.lowercased().range(of: query) else { return nil }

        let lowerBound = text.index(range.lowerBound, offsetBy: -50, limitedBy: text.startIndex) ?? text.startIndex
        let upperBound = text.index(range.upperBound, offsetBy: 50, limitedBy: text.endIndex) ?? text.endIndex

        var context = String(text[lowerBound..<upperBound])
        if lowerBound != text.startIndex { context = "..." + context }
        if upperBound != text.endIndex { context = context + "..." }

        return context
    }
}
