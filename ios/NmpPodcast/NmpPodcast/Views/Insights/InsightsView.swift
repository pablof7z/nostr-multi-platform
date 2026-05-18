import SwiftUI
import SwiftData
import AVFoundation
import OSLog

struct InsightsView: View {
    @Bindable var audioService: AudioService
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Insight.createdAt, order: .reverse) private var insights: [Insight]
    @State private var playingThoughtID: UUID?
    @State private var thoughtPlayer: AVPlayer?

    var body: some View {
        NavigationStack {
            Group {
                if insights.isEmpty {
                    emptyState
                } else {
                    insightsList
                }
            }
            .navigationTitle("Insights")
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Spacer()

            Image(systemName: "lightbulb")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text("No Insights Yet")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Capture your thoughts while listening to podcasts using the lightbulb button in the player.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            Spacer()
        }
    }

    private var insightsList: some View {
        ScrollView {
            LazyVStack(spacing: 16) {
                ForEach(insights) { insight in
                    InsightCard(
                        insight: insight,
                        isPlayingThought: playingThoughtID == insight.id,
                        onPlayThought: { playThought(insight) },
                        onPlayExcerpt: { playExcerpt(insight) },
                        onDelete: { deleteInsight(insight) }
                    )
                }
            }
            .padding()
        }
    }

    private func playThought(_ insight: Insight) {
        guard let audioPath = insight.thoughtAudioPath else { return }

        // Stop any currently playing thought
        thoughtPlayer?.pause()

        if playingThoughtID == insight.id {
            playingThoughtID = nil
            return
        }

        let player = AVPlayer(url: audioPath)
        thoughtPlayer = player

        // Observe when playback ends
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
        guard let episode = insight.episode else { return }

        // Play the episode from the excerpt start time
        Task {
            await audioService.play(episode: episode, from: insight.excerptStartTime)
        }
    }

    private func deleteInsight(_ insight: Insight) {
        // Delete audio file if it exists
        if let audioPath = insight.thoughtAudioPath {
            try? FileManager.default.removeItem(at: audioPath)
        }

        modelContext.delete(insight)
        do {
            try modelContext.save()
        } catch {
            Logger.database.error("Failed to save context after deleting insight: \(error)")
        }
    }
}

struct InsightCard: View {
    let insight: Insight
    let isPlayingThought: Bool
    let onPlayThought: () -> Void
    let onPlayExcerpt: () -> Void
    let onDelete: () -> Void

    @State private var showingDeleteConfirmation = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header with episode info
            if let episode = insight.episode {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        if let podcastTitle = episode.podcast?.title {
                            Text(podcastTitle)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Text(episode.title)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .lineLimit(1)
                    }

                    Spacer()

                    Text(insight.createdAt, style: .date)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }

            // User's thought
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Image(systemName: "quote.opening")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("Your thought")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text(insight.thoughtText)
                    .font(.body)

                if insight.thoughtAudioPath != nil {
                    Button {
                        onPlayThought()
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: isPlayingThought ? "stop.circle.fill" : "play.circle.fill")
                            Text(isPlayingThought ? "Stop" : "Play recording")
                        }
                        .font(.caption)
                        .foregroundStyle(Color.accentColor)
                    }
                }
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.accentColor.opacity(0.1))
            .clipShape(RoundedRectangle(cornerRadius: 12))

            // Matched excerpt
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Image(systemName: "text.quote")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("From the podcast")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text(formatTimeRange(insight.excerptStartTime, insight.excerptEndTime))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }

                Text(insight.excerptText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(4)

                Button {
                    onPlayExcerpt()
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "play.circle.fill")
                        Text("Listen to this part")
                    }
                    .font(.caption)
                    .foregroundStyle(Color.accentColor)
                }
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(.systemGray6))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
        .padding()
        .background(Color(.systemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 16))
        .shadow(color: .black.opacity(0.05), radius: 8, x: 0, y: 2)
        .contextMenu {
            Button(role: .destructive) {
                showingDeleteConfirmation = true
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
        .confirmationDialog("Delete this insight?", isPresented: $showingDeleteConfirmation) {
            Button("Delete", role: .destructive) {
                onDelete()
            }
        }
    }

    private func formatTimeRange(_ start: TimeInterval, _ end: TimeInterval) -> String {
        "\(formatTime(start)) - \(formatTime(end))"
    }

    private func formatTime(_ time: TimeInterval) -> String {
        let minutes = Int(time) / 60
        let seconds = Int(time) % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

#Preview {
    InsightsView(audioService: AudioService())
        .modelContainer(for: [Insight.self, Episode.self], inMemory: true)
}
