import SwiftUI

struct EpisodeRow: View {
    let episode: Episode
    var audioService: AudioService?
    var processingQueue: ProcessingQueue?
    var onPlay: ((Episode) -> Void)?
    @State private var showingDetails = false

    var body: some View {
        Button {
            playEpisode()
        } label: {
            HStack(alignment: .top, spacing: 12) {
                CachedAsyncImage(url: episode.podcast?.artworkURL) {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(Color.gray.opacity(0.2))
                }
                .aspectRatio(contentMode: .fill)
                .frame(width: 56, height: 56)
                .clipShape(RoundedRectangle(cornerRadius: 8))

                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(episode.podcast?.title ?? "Unknown")
                            .font(.caption)
                            .fontWeight(.semibold)
                            .foregroundStyle(.primary)

                        Text(episode.publishedAt.relativeFormatted)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    Text(episode.title)
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .lineLimit(2)
                        .foregroundStyle(.primary)

                    if let summary = episode.aiSummary {
                        Text(summary)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .lineLimit(3)
                    }

                    // Progress indicator for active jobs
                    if let activeJob = activeJob {
                        HStack(spacing: 8) {
                            ProgressView()
                                .scaleEffect(0.7)
                            Text(activeJob.type.rawValue)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                        }
                        .padding(.top, 2)
                    } else {
                        HStack(spacing: 12) {
                            // Download button
                            Button {
                                processingQueue?.enqueueDownload(episode: episode)
                            } label: {
                                downloadIcon
                                    .font(.system(size: 16))
                            }
                            .buttonStyle(.plain)
                            .disabled(episode.downloadState == .downloaded || episode.downloadState == .downloading)

                            Text(episode.duration.formatted)
                                .font(.caption)
                                .foregroundStyle(.secondary)

                            if !episode.hasBeenPlayed && episode.playbackPosition == 0 {
                                Text("New")
                                    .font(.caption)
                                    .foregroundStyle(.orange)
                            }

                            Spacer()

                            if !episode.insights.isEmpty {
                                HStack(spacing: 2) {
                                    Image(systemName: "lightbulb.fill")
                                        .foregroundStyle(.yellow)
                                    Text("\(episode.insights.count)")
                                }
                                .font(.caption)
                            }

                            if isPlaying {
                                Image(systemName: "waveform")
                                    .font(.caption)
                                    .foregroundStyle(Color.accentColor)
                                    .symbolEffect(.variableColor.iterative)
                            }
                        }
                        .padding(.top, 2)
                    }
                }
            }
            .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
        .opacity(episode.downloadState == .downloaded ? 1.0 : 0.5)
        .contextMenu {
            Button {
                showingDetails = true
            } label: {
                Label("Episode Details", systemImage: "info.circle")
            }

            if !episode.insights.isEmpty {
                Button {
                    showingDetails = true
                } label: {
                    Label("View \(episode.insights.count) Insight\(episode.insights.count == 1 ? "" : "s")", systemImage: "lightbulb")
                }
            }
        }
        .sheet(isPresented: $showingDetails) {
            if let audioService = audioService {
                NavigationStack {
                    EpisodeDetailView(episode: episode, audioService: audioService)
                        .toolbar {
                            ToolbarItem(placement: .topBarLeading) {
                                Button("Done") {
                                    showingDetails = false
                                }
                            }
                        }
                }
            }
        }
    }

    private var isPlaying: Bool {
        guard let audioService = audioService else { return false }
        return audioService.currentEpisode?.id == episode.id &&
               audioService.playbackState == .playing
    }

    private var activeJob: QueueJob? {
        processingQueue?.activeJobs.first { $0.episodeID == episode.id }
    }

    @ViewBuilder
    private var downloadIcon: some View {
        switch episode.downloadState {
        case .notDownloaded:
            Image(systemName: "arrow.down.circle")
                .foregroundStyle(.secondary)
        case .downloading:
            ProgressView()
                .scaleEffect(0.7)
        case .downloaded:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .failed:
            Image(systemName: "exclamationmark.circle")
                .foregroundStyle(.red)
        }
    }

    private func playEpisode() {
        if let audioService = audioService {
            if isPlaying {
                audioService.pause()
            } else if audioService.currentEpisode?.id == episode.id {
                audioService.resume()
            } else {
                // Start background download when playing (if not already downloaded)
                if episode.downloadState == .notDownloaded {
                    processingQueue?.enqueueDownload(episode: episode)
                } else if episode.downloadState == .downloaded && episode.transcript == nil {
                    // Already downloaded but not transcribed - start transcription
                    processingQueue?.enqueueTranscription(episode: episode)
                }

                Task {
                    let position = episode.playbackPosition > 0 ? episode.playbackPosition : nil
                    await audioService.play(episode: episode, from: position)
                }
            }
        }
        onPlay?(episode)
    }
}

extension TimeInterval {
    var formatted: String {
        let hours = Int(self) / 3600
        let minutes = (Int(self) % 3600) / 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        return "\(minutes)m"
    }
}

extension Date {
    var relativeFormatted: String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: self, relativeTo: Date())
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
        aiSummary: "Naval shares his framework for building wealth without luck, the four types of luck, and why specific knowledge compounds.",
        audioURL: URL(string: "https://example.com/audio.mp3")!,
        duration: 8040,
        publishedAt: Date().addingTimeInterval(-7200)
    )

    return List {
        EpisodeRow(episode: episode, audioService: AudioService(), processingQueue: ServiceContainer.shared.processingQueue)
    }
    .listStyle(.plain)
}
