import SwiftUI
import SwiftData
import AVFoundation

struct EpisodeDetailView: View {
    let episode: Episode
    var audioService: AudioService
    @Environment(\.dismiss) private var dismiss
    @State private var playingThoughtID: UUID?
    @State private var thoughtPlayer: AVPlayer?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                // Episode header
                episodeHeader

                // AI Summary
                if let summary = episode.aiSummary {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Summary")
                            .font(.headline)
                        Text(summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }

                // Insights section
                if !episode.insights.isEmpty {
                    insightsSection
                }

                // Description
                if let description = episode.descriptionText, !description.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Description")
                            .font(.headline)
                        Text(description)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding()
        }
        .navigationTitle("Episode")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    playEpisode()
                } label: {
                    Image(systemName: "play.circle.fill")
                        .font(.title2)
                }
            }
        }
    }

    private var episodeHeader: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 16) {
                CachedAsyncImage(url: episode.podcast?.artworkURL) {
                    RoundedRectangle(cornerRadius: 12)
                        .fill(Color.gray.opacity(0.2))
                }
                .aspectRatio(contentMode: .fill)
                .frame(width: 100, height: 100)
                .clipShape(RoundedRectangle(cornerRadius: 12))

                VStack(alignment: .leading, spacing: 4) {
                    if let podcastTitle = episode.podcast?.title {
                        Text(podcastTitle)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Text(episode.title)
                        .font(.title3)
                        .fontWeight(.semibold)

                    HStack(spacing: 8) {
                        Text(episode.publishedAt.relativeFormatted)
                        Text("•")
                        Text(episode.duration.formatted)
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var insightsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "lightbulb.fill")
                    .foregroundStyle(.yellow)
                Text("Your Insights")
                    .font(.headline)
                Spacer()
                Text("\(episode.insights.count)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            ForEach(episode.insights.sorted { $0.createdAt > $1.createdAt }) { insight in
                EpisodeInsightCard(
                    insight: insight,
                    isPlayingThought: playingThoughtID == insight.id,
                    onPlayThought: { playThought(insight) },
                    onPlayExcerpt: { playExcerpt(insight) }
                )
            }
        }
    }

    private func playEpisode() {
        Task {
            let position = episode.playbackPosition > 0 ? episode.playbackPosition : nil
            await audioService.play(episode: episode, from: position)
        }
    }

    private func playThought(_ insight: Insight) {
        guard let audioPath = insight.thoughtAudioPath else { return }

        thoughtPlayer?.pause()

        if playingThoughtID == insight.id {
            playingThoughtID = nil
            return
        }

        let player = AVPlayer(url: audioPath)
        thoughtPlayer = player

        NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime,
            object: player.currentItem,
            queue: .main
        ) { [weak audioService] _ in
            Task { @MainActor in
                playingThoughtID = nil
            }
        }

        player.play()
        playingThoughtID = insight.id
    }

    private func playExcerpt(_ insight: Insight) {
        Task {
            await audioService.play(episode: episode, from: insight.excerptStartTime)
        }
    }
}

struct EpisodeInsightCard: View {
    let insight: Insight
    let isPlayingThought: Bool
    let onPlayThought: () -> Void
    let onPlayExcerpt: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            // User's thought
            HStack(alignment: .top) {
                Image(systemName: "quote.opening")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Text(insight.thoughtText)
                    .font(.subheadline)

                Spacer()

                if insight.thoughtAudioPath != nil {
                    Button {
                        onPlayThought()
                    } label: {
                        Image(systemName: isPlayingThought ? "stop.circle.fill" : "play.circle.fill")
                            .font(.title3)
                            .foregroundStyle(Color.accentColor)
                    }
                }
            }

            // Excerpt preview
            VStack(alignment: .leading, spacing: 4) {
                Text(insight.excerptText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                Button {
                    onPlayExcerpt()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "play.fill")
                        Text("Listen at \(formatTime(insight.excerptStartTime))")
                    }
                    .font(.caption)
                    .foregroundStyle(Color.accentColor)
                }
            }
            .padding(10)
            .background(Color(.systemGray6))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            Text(insight.createdAt, style: .date)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding()
        .background(Color(.systemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .shadow(color: .black.opacity(0.05), radius: 4, x: 0, y: 2)
    }

    private func formatTime(_ time: TimeInterval) -> String {
        let minutes = Int(time) / 60
        let seconds = Int(time) % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

#Preview {
    let podcast = Podcast(
        feedURL: URL(string: "https://example.com/feed")!,
        title: "Tim Ferriss",
        author: "Tim Ferriss"
    )
    let episode = Episode(
        podcast: podcast,
        guid: "123",
        title: "Naval Ravikant — The Angel Philosopher",
        aiSummary: "Naval shares his framework for building wealth without luck.",
        audioURL: URL(string: "https://example.com/audio.mp3")!,
        duration: 8040
    )

    return NavigationStack {
        EpisodeDetailView(episode: episode, audioService: AudioService())
    }
}
