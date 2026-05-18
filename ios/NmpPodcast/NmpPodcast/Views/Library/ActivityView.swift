import SwiftUI
import SwiftData

struct ActivityView: View {
    @Query(sort: \Episode.publishedAt, order: .reverse) private var episodes: [Episode]
    @State private var filter: StatusFilter = .all
    @State private var showingQueue = false
    private let processingQueue = ServiceContainer.shared.processingQueue

    enum StatusFilter: String, CaseIterable {
        case all = "All"
        case downloaded = "Downloaded"
        case transcribed = "Transcribed"
        case inProgress = "In Progress"
        case unplayed = "Unplayed"
    }

    private var filteredEpisodes: [Episode] {
        switch filter {
        case .all:
            return episodes
        case .downloaded:
            return episodes.filter { $0.downloadState == .downloaded }
        case .transcribed:
            return episodes.filter { $0.transcript != nil }
        case .inProgress:
            return episodes.filter { $0.playbackPosition > 0 && !$0.hasBeenPlayed }
        case .unplayed:
            return episodes.filter { !$0.hasBeenPlayed && $0.playbackPosition == 0 }
        }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Filter picker
                Picker("Filter", selection: $filter) {
                    ForEach(StatusFilter.allCases, id: \.self) { filter in
                        Text(filter.rawValue).tag(filter)
                    }
                }
                .pickerStyle(.segmented)
                .padding()

                // Stats summary
                statsBar

                // Episode list
                if filteredEpisodes.isEmpty {
                    emptyState
                } else {
                    List {
                        ForEach(filteredEpisodes) { episode in
                            EpisodeStatusRow(episode: episode)
                        }
                    }
                    .listStyle(.plain)
                }
            }
            .navigationTitle("Activity")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        showingQueue = true
                    } label: {
                        HStack(spacing: 4) {
                            if !processingQueue.activeJobs.isEmpty {
                                Text("\(processingQueue.activeJobs.count)")
                                    .font(.caption)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.accentColor)
                                    .foregroundStyle(.white)
                                    .clipShape(Capsule())
                            }
                            Image(systemName: "arrow.down.circle")
                        }
                    }
                }
            }
            .sheet(isPresented: $showingQueue) {
                QueueView(processingQueue: processingQueue)
            }
        }
    }

    private var statsBar: some View {
        HStack(spacing: 16) {
            StatBadge(
                icon: "arrow.down.circle.fill",
                count: episodes.filter { $0.downloadState == .downloaded }.count,
                label: "Downloaded",
                color: .blue
            )

            StatBadge(
                icon: "text.bubble.fill",
                count: episodes.filter { $0.transcript != nil }.count,
                label: "Transcribed",
                color: .green
            )

            StatBadge(
                icon: "checkmark.circle.fill",
                count: episodes.filter { $0.hasBeenPlayed }.count,
                label: "Played",
                color: .purple
            )

            StatBadge(
                icon: "sparkles",
                count: episodes.filter { $0.aiSummary != nil }.count,
                label: "Summarized",
                color: .orange
            )
        }
        .padding(.horizontal)
        .padding(.bottom)
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No episodes")
                .font(.headline)
            Text("Episodes matching your filter will appear here.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Spacer()
        }
        .padding()
    }
}

struct StatBadge: View {
    let icon: String
    let count: Int
    let label: String
    let color: Color

    var body: some View {
        VStack(spacing: 4) {
            HStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.caption)
                Text("\(count)")
                    .font(.headline)
            }
            .foregroundStyle(color)

            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
    }
}

struct EpisodeStatusRow: View {
    let episode: Episode

    private var downloadIcon: String {
        switch episode.downloadState {
        case .notDownloaded:
            return "arrow.down.circle"
        case .downloading:
            return "arrow.down.circle.dotted"
        case .downloaded:
            return "arrow.down.circle.fill"
        case .failed:
            return "exclamationmark.circle"
        }
    }

    private var downloadColor: Color {
        switch episode.downloadState {
        case .notDownloaded:
            return .secondary
        case .downloading:
            return .blue
        case .downloaded:
            return .blue
        case .failed:
            return .red
        }
    }

    private var progressPercent: Int {
        guard episode.duration > 0 else { return 0 }
        return Int((episode.playbackPosition / episode.duration) * 100)
    }

    var body: some View {
        HStack(spacing: 12) {
            // Podcast artwork
            CachedAsyncImage(url: episode.podcast?.artworkURL) {
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color.gray.opacity(0.2))
            }
            .aspectRatio(contentMode: .fill)
            .frame(width: 50, height: 50)
            .clipShape(RoundedRectangle(cornerRadius: 6))

            // Episode info
            VStack(alignment: .leading, spacing: 4) {
                Text(episode.title)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .lineLimit(2)

                Text(episode.podcast?.title ?? "Unknown Podcast")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                // Progress bar for in-progress episodes
                if episode.playbackPosition > 0 && !episode.hasBeenPlayed {
                    GeometryReader { geometry in
                        ZStack(alignment: .leading) {
                            Rectangle()
                                .fill(Color.secondary.opacity(0.2))
                            Rectangle()
                                .fill(Color.accentColor)
                                .frame(width: geometry.size.width * (episode.playbackPosition / max(episode.duration, 1)))
                        }
                    }
                    .frame(height: 3)
                    .clipShape(Capsule())
                }
            }

            Spacer()

            // Status indicators
            VStack(alignment: .trailing, spacing: 6) {
                // Download status
                HStack(spacing: 4) {
                    Image(systemName: downloadIcon)
                        .foregroundStyle(downloadColor)
                    if episode.downloadState == .downloaded {
                        Text("Downloaded")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                // Transcript status
                HStack(spacing: 4) {
                    Image(systemName: episode.transcript != nil ? "text.bubble.fill" : "text.bubble")
                        .foregroundStyle(episode.transcript != nil ? .green : .secondary)
                    if episode.transcript != nil {
                        Text("Transcribed")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                // Playback status
                HStack(spacing: 4) {
                    if episode.hasBeenPlayed {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(.purple)
                        Text("Played")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    } else if episode.playbackPosition > 0 {
                        Image(systemName: "play.circle.fill")
                            .foregroundStyle(Color.accentColor)
                        Text("\(progressPercent)%")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .font(.caption)
        }
        .padding(.vertical, 4)
    }
}

#Preview {
    ActivityView()
        .modelContainer(for: [Episode.self, Podcast.self], inMemory: true)
}
