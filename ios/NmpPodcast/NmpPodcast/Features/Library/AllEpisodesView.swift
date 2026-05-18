import SwiftUI

// MARK: - AllEpisodesFilter (verbatim from Podcastr)
//
// Copied byte-for-byte from:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Library/AllEpisodesView.swift
// Matches section: lines 1-45.

enum AllEpisodesFilter: String, CaseIterable, Identifiable {
    case all
    case unplayed
    case inProgress
    case downloaded
    case starred

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all:        return "All"
        case .unplayed:   return "Unplayed"
        case .inProgress: return "In Progress"
        case .downloaded: return "Downloaded"
        case .starred:    return "Starred"
        }
    }

    var systemImage: String? {
        switch self {
        case .all:        return nil
        case .unplayed:   return "circle.fill"
        case .inProgress: return "circle.lefthalf.filled"
        case .downloaded: return "arrow.down.circle.fill"
        case .starred:    return "star.fill"
        }
    }
}

// MARK: - LibraryEpisodeRoute

struct LibraryEpisodeRoute: Hashable {
    let episodeID: UUID
    let subscriptionID: UUID
    let title: String
}

// MARK: - AllEpisodesView
//
// T-podcast-ios-3 (Step 2): Wire AppStateStore.allPodcasts (kernel-backed) into
// the Library tab. Rows are tappable NavigationLinks to ShowDetailView.
// The kernel delivers: id, title, author, artwork_url, episode_count.
//
// The verbatim Podcastr AllEpisodesView shows all-episodes-across-subscriptions
// (needs T-podcast-gap-002 episode data). This stub intentionally shows the
// podcast grid instead — an honest, kernel-backed representation.

struct AllEpisodesView: View {
    @Environment(AppStateStore.self) private var store
    @State private var showAddShow = false

    var body: some View {
        let podcasts = store.allPodcasts
        Group {
            if podcasts.isEmpty {
                ContentUnavailableView(
                    "No Podcasts",
                    systemImage: "tray",
                    description: Text("Subscribe to podcasts to build your library.")
                )
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            showAddShow = true
                        } label: {
                            Image(systemName: "plus")
                        }
                        .accessibilityLabel("Add podcast")
                        .accessibilityIdentifier("addPodcastButton")
                    }
                }
            } else {
                List {
                    Section {
                        ForEach(podcasts) { podcast in
                            NavigationLink(value: podcast) {
                                podcastRow(podcast)
                            }
                            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                Button(role: .destructive) {
                                    store.deletePodcast(podcastID: podcast.id)
                                } label: {
                                    Label("Unsubscribe", systemImage: "minus.circle")
                                }
                            }
                        }
                    } header: {
                        Text("Your Podcasts (\(podcasts.count))")
                    }
                }
                .listStyle(.plain)
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            showAddShow = true
                        } label: {
                            Image(systemName: "plus")
                        }
                        .accessibilityLabel("Add podcast")
                        .accessibilityIdentifier("addPodcastButton")
                    }
                }
            }
        }
        .navigationTitle("Library")
        .navigationBarTitleDisplayMode(.large)
        .navigationDestination(for: Podcast.self) { podcast in
            ShowDetailView(podcast: podcast)
        }
        .sheet(isPresented: $showAddShow) {
            AddShowSheet()
        }
    }

    @ViewBuilder
    private func podcastRow(_ podcast: Podcast) -> some View {
        HStack(spacing: 12) {
            CachedAsyncImageShim(url: podcast.imageURL)
                .frame(width: 48, height: 48)
                .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 4) {
                Text(podcast.title.isEmpty ? "Untitled" : podcast.title)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .lineLimit(1)

                if !podcast.author.isEmpty {
                    Text(podcast.author)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Text("\(podcast.episodeCount) episodes")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }
}

/// Inline shim for CachedAsyncImage since Kingfisher may not be configured yet.
private struct CachedAsyncImageShim: View {
    let url: URL?

    var body: some View {
        if let url {
            AsyncImage(url: url) { image in
                image.resizable().aspectRatio(contentMode: .fill)
            } placeholder: {
                RoundedRectangle(cornerRadius: 8).fill(Color.gray.opacity(0.2))
            }
        } else {
            RoundedRectangle(cornerRadius: 8).fill(Color.gray.opacity(0.2))
        }
    }
}
