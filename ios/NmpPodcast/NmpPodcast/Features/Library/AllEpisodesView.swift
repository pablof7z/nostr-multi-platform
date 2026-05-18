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
// T-podcast-gap-002: The verbatim AllEpisodesView body references Episode
// computed properties (played, isInProgress, downloadState, isStarred) and
// Podcast.accentColor that require a full kernel episode data model not yet
// exposed. This stub renders Podcastr's library empty state with a
// kernel-backed podcast count.
//
// Wire to KernelModel.library for the podcast list. Episodes populate once
// the kernel exposes nmp_app_podcast_snapshot with episode rows.

struct AllEpisodesView: View {
    @EnvironmentObject private var kernelModel: KernelModel

    var body: some View {
        let podcasts = kernelModel.library.podcasts
        Group {
            if podcasts.isEmpty {
                ContentUnavailableView(
                    "No episodes yet.",
                    systemImage: "tray",
                    description: Text("Subscribe to podcasts from the Home tab to see episodes here.")
                )
            } else {
                List {
                    Section {
                        ForEach(podcasts) { podcast in
                            HStack(spacing: 12) {
                                CachedAsyncImageShim(url: podcast.artworkURL)
                                    .frame(width: 48, height: 48)
                                    .clipShape(RoundedRectangle(cornerRadius: 8))

                                VStack(alignment: .leading, spacing: 4) {
                                    Text(podcast.title)
                                        .font(.subheadline)
                                        .fontWeight(.medium)

                                    if !podcast.author.isEmpty {
                                        Text(podcast.author)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }

                                    Text("\(podcast.episodeCount) episodes")
                                        .font(.caption2)
                                        .foregroundStyle(.tertiary)
                                }
                            }
                            .padding(.vertical, 4)
                        }
                    } header: {
                        Text("Your Podcasts")
                    }
                }
                .listStyle(.plain)
            }
        }
        .navigationTitle("Library")
        .navigationBarTitleDisplayMode(.large)
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
