import SwiftUI

extension DiscoverView {
    // MARK: - Hero Section

    @ViewBuilder
    var heroSection: some View {
        if let hero = personalizedHero, !hero.recommendations.isEmpty {
            // Personalized hero
            VStack(alignment: .leading, spacing: 12) {
                Text("Because you listened to")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal)

                Text("\"\(hero.sourceEpisodeTitle)\"")
                    .font(.headline)
                    .lineLimit(2)
                    .padding(.horizontal)

                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 16) {
                        ForEach(hero.recommendations) { rec in
                            RecommendationCard(podcast: rec.podcast, reason: rec.reason)
                                .onTapGesture {
                                    selectedPodcast = rec.podcast
                                }
                        }
                    }
                    .padding(.horizontal)
                }
            }
        } else if let featured = trendingPodcasts.first {
            // Trending hero fallback
            VStack(alignment: .leading, spacing: 12) {
                Text("Trending Now")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal)

                FeaturedTrendingCard(podcast: featured, rank: 1)
                    .padding(.horizontal)
                    .onTapGesture {
                        selectedPodcast = featured
                    }
            }
        } else if isLoadingTrending {
            ProgressView()
                .frame(maxWidth: .infinity)
                .frame(height: 200)
        }
    }

    // MARK: - For You Section

    var forYouSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            DiscoverySectionHeader(title: "For You", subtitle: "Based on your listening history")

            if isLoadingRecommendations {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .frame(height: 200)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 16) {
                        ForEach(recommendations) { rec in
                            RecommendationCard(podcast: rec.podcast, reason: rec.reason)
                                .onTapGesture {
                                    selectedPodcast = rec.podcast
                                }
                        }
                    }
                    .padding(.horizontal)
                }
            }
        }
    }

    // MARK: - Trending Section

    var trendingSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            DiscoverySectionHeader(title: "Trending", subtitle: nil) {
                showAllTrending = true
            }

            if isLoadingTrending {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .frame(height: 200)
            } else {
                // Skip first if we showed it in hero
                let displayPodcasts = personalizedHero != nil ? Array(trendingPodcasts.prefix(5)) : Array(trendingPodcasts.dropFirst().prefix(4))
                let startRank = personalizedHero != nil ? 1 : 2

                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                    ForEach(Array(displayPodcasts.enumerated()), id: \.element.id) { index, podcast in
                        VStack(alignment: .leading, spacing: 8) {
                            TrendingGridCard(podcast: podcast, rank: startRank + index)
                                .aspectRatio(1, contentMode: .fit)

                            Text(podcast.title)
                                .font(.caption)
                                .fontWeight(.medium)
                                .lineLimit(2)

                            if let author = podcast.author {
                                Text(author)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        .onTapGesture {
                            selectedPodcast = podcast
                        }
                    }
                }
                .padding(.horizontal)
            }
        }
    }

    // MARK: - Categories Section

    var categoriesSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            DiscoverySectionHeader(title: "Browse by Category", subtitle: nil) {
                showAllCategories = true
            }

            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                ForEach(PodcastIndexCategory.all.prefix(6)) { category in
                    NavigationLink {
                        CategoryDetailView(category: category)
                    } label: {
                        CategoryCard(category: category)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal)
        }
    }

    // MARK: - Topics Section

    var topicsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            DiscoverySectionHeader(title: "Your Topics", subtitle: "Explore content you love")

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(userTopics, id: \.self) { topic in
                        NavigationLink {
                            TopicSearchView(topic: topic)
                        } label: {
                            TopicPill(topic: topic, isSelected: false)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal)
            }
        }
    }

    // MARK: - Search Results

    var searchResultsSection: some View {
        VStack(alignment: .leading, spacing: 20) {
            if isSearching && isSearchingEpisodes {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .padding(.top, 40)
            } else if searchResults.isEmpty && episodeSearchResults.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "magnifyingglass")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text("No results for \"\(searchText)\"")
                        .font(.headline)
                    Text("Try a different search term")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 40)
            } else {
                // Episodes from your library (if any)
                if !episodeSearchResults.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Episodes in Your Library")
                                .font(.headline)
                            Spacer()
                            if isSearchingEpisodes {
                                ProgressView()
                                    .scaleEffect(0.8)
                            }
                        }
                        .padding(.horizontal)

                        ForEach(episodeSearchResults.prefix(5)) { result in
                            EpisodeSearchRow(episode: result.episode, matchContext: result.matchContext)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    selectedEpisode = result.episode
                                }
                        }
                    }
                }

                // Podcasts section
                if !searchResults.isEmpty {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Podcasts")
                                .font(.headline)
                            Spacer()
                            if isSearching {
                                ProgressView()
                                    .scaleEffect(0.8)
                            }
                        }
                        .padding(.horizontal)

                        ForEach(searchResults.prefix(10)) { podcast in
                            PodcastSearchRow(podcast: podcast)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    selectedPodcast = podcast
                                }
                        }
                    }
                }
            }
        }
    }

    // MARK: - Add by URL

    var addByURLSection: some View {
        VStack(spacing: 12) {
            Divider()
                .padding(.horizontal)

            Button {
                showAddByURL = true
            } label: {
                HStack {
                    Image(systemName: "link")
                    Text("Add by RSS URL")
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)
            }
            .padding(.vertical, 8)
        }
    }
}
