import SwiftUI
import SwiftData

struct TranscriptView: View {
    let episode: Episode
    @Bindable var audioService: AudioService
    @Environment(\.modelContext) private var modelContext

    private let processingQueue = ServiceContainer.shared.processingQueue

    private var sortedChunks: [TranscriptChunk] {
        episode.transcript?.chunks.sorted { $0.startTime < $1.startTime } ?? []
    }

    private var currentChunkIndex: Int? {
        let currentTime = audioService.currentTime
        return sortedChunks.firstIndex { chunk in
            currentTime >= chunk.startTime && currentTime < chunk.endTime
        }
    }

    private var isTranscribing: Bool {
        processingQueue.jobs.contains { job in
            job.episodeID == episode.id &&
            job.type == .transcribe &&
            (job.status == .queued || job.status == .running)
        }
    }

    private var isSummarizing: Bool {
        processingQueue.jobs.contains { job in
            job.episodeID == episode.id &&
            job.type == .summarize &&
            (job.status == .queued || job.status == .running)
        }
    }

    var body: some View {
        Group {
            if let transcript = episode.transcript {
                transcriptContent(transcript)
            } else if isTranscribing {
                transcribingView
            } else {
                noTranscriptView
            }
        }
        .navigationTitle("Transcript")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func transcriptContent(_ transcript: Transcript) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    // Summary section
                    if isSummarizing {
                        HStack {
                            ProgressView()
                            Text("Generating summary...")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                        .padding()
                        .frame(maxWidth: .infinity)
                        .background(Color.secondary.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    } else if let summary = episode.aiSummary {
                        VStack(alignment: .leading, spacing: 8) {
                            Label("AI Summary", systemImage: "sparkles")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Text(summary)
                                .font(.subheadline)
                        }
                        .padding()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.accentColor.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    } else {
                        Button {
                            processingQueue.enqueueSummarization(episode: episode, text: transcript.fullText)
                        } label: {
                            Label("Generate AI Summary", systemImage: "sparkles")
                                .font(.subheadline)
                        }
                        .buttonStyle(.bordered)
                    }

                    ForEach(Array(sortedChunks.enumerated()), id: \.element.id) { index, chunk in
                        ChunkRow(
                            chunk: chunk,
                            isActive: index == currentChunkIndex,
                            onTap: {
                                audioService.seek(to: chunk.startTime)
                            }
                        )
                        .id(chunk.id)
                    }
                }
                .padding()
            }
            .onChange(of: currentChunkIndex) { oldValue, newValue in
                if let newValue = newValue, let chunk = sortedChunks[safe: newValue] {
                    withAnimation {
                        proxy.scrollTo(chunk.id, anchor: .center)
                    }
                }
            }
        }
    }

    private var transcribingView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .scaleEffect(1.5)

            Text("Transcribing...")
                .font(.headline)

            Text("This may take a few minutes for longer episodes.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding()
    }

    private var noTranscriptView: some View {
        VStack(spacing: 20) {
            Image(systemName: "text.bubble")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text("No Transcript")
                .font(.title2)
                .fontWeight(.semibold)

            if episode.downloadState != .downloaded {
                Text("Download the episode first to generate a transcript.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)

                Button {
                    processingQueue.enqueueDownload(episode: episode)
                } label: {
                    Label("Download Episode", systemImage: "arrow.down.circle")
                        .padding(.horizontal, 20)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
            } else {
                Text("Generate a transcript to read along and search this episode.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)

                Button {
                    processingQueue.enqueueTranscription(episode: episode)
                } label: {
                    Label("Generate Transcript", systemImage: "waveform")
                        .padding(.horizontal, 20)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding()
    }
}

struct ChunkRow: View {
    let chunk: TranscriptChunk
    let isActive: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(alignment: .top, spacing: 12) {
                Text(formatTime(chunk.startTime))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(width: 50, alignment: .trailing)

                Text(chunk.text)
                    .font(.body)
                    .foregroundStyle(isActive ? .primary : .secondary)
                    .fontWeight(isActive ? .medium : .regular)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 12)
            .background(isActive ? Color.accentColor.opacity(0.1) : Color.clear)
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
    }

    private func formatTime(_ time: TimeInterval) -> String {
        let totalSeconds = Int(time)
        let minutes = totalSeconds / 60
        let seconds = totalSeconds % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
