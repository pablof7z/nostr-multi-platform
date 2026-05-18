import SwiftUI

struct ChaptersPanel: View {
    let episode: Episode
    @Bindable var audioService: AudioService
    let onClose: () -> Void

    @State private var searchText = ""
    @State private var isSearching = false
    @State private var searchResult: SearchResult?

    private let aiService = ServiceContainer.shared.aiService
    private let processingQueue = ServiceContainer.shared.processingQueue

    private var sortedChapters: [Chapter] {
        episode.transcript?.chapters.sorted { $0.chapterIndex < $1.chapterIndex } ?? []
    }

    private var currentChapterIndex: Int? {
        let currentTime = audioService.currentTime
        return sortedChapters.firstIndex { chapter in
            currentTime >= chapter.startTime && currentTime < chapter.endTime
        }
    }

    private var filteredChapters: [Chapter] {
        guard !searchText.isEmpty else { return sortedChapters }
        let query = searchText.lowercased()
        return sortedChapters.filter {
            $0.title.lowercased().contains(query) ||
            $0.summary.lowercased().contains(query)
        }
    }

    private var isExtractingChapters: Bool {
        processingQueue.jobs.contains { job in
            job.episodeID == episode.id &&
            job.type == .extractChapters &&
            (job.status == .queued || job.status == .running)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            searchBar
            Divider()
            content
        }
        .background(Color(.systemBackground))
    }

    private var header: some View {
        HStack {
            Text("Chapters")
                .font(.headline)

            Spacer()

            Button {
                onClose()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .font(.title2)
                    .foregroundStyle(.secondary)
            }
        }
        .padding()
    }

    private var searchBar: some View {
        HStack {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)

            TextField("Search or ask...", text: $searchText)
                .textFieldStyle(.plain)
                .onSubmit {
                    performSearch()
                }

            if isSearching {
                ProgressView()
                    .scaleEffect(0.8)
            } else if !searchText.isEmpty {
                Button {
                    searchText = ""
                    searchResult = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(10)
        .background(Color(.secondarySystemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .padding(.horizontal)
        .padding(.bottom, 12)
    }

    @ViewBuilder
    private var content: some View {
        if let result = searchResult {
            searchResultView(result)
        } else if sortedChapters.isEmpty {
            emptyStateView
        } else {
            chaptersList
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Spacer()

            if isExtractingChapters {
                ProgressView()
                    .scaleEffect(1.2)
                Text("Generating chapters...")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else if episode.transcript == nil {
                Image(systemName: "list.bullet.rectangle")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)
                Text("No transcript yet")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "list.bullet.rectangle")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)
                Text("No chapters available")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Button {
                    processingQueue.enqueueChapterExtraction(episode: episode)
                } label: {
                    Label("Generate Chapters", systemImage: "sparkles")
                }
                .buttonStyle(.bordered)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    private var chaptersList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(Array(filteredChapters.enumerated()), id: \.element.id) { index, chapter in
                        let originalIndex = sortedChapters.firstIndex(where: { $0.id == chapter.id })
                        ChapterRow(
                            chapter: chapter,
                            isActive: originalIndex == currentChapterIndex,
                            onTap: {
                                audioService.seek(to: chapter.startTime)
                            }
                        )
                        .id(chapter.id)
                    }
                }
                .padding(.vertical, 8)
            }
            .onChange(of: currentChapterIndex) { _, newValue in
                if let newValue, searchText.isEmpty, let chapter = sortedChapters[safe: newValue] {
                    withAnimation {
                        proxy.scrollTo(chapter.id, anchor: .center)
                    }
                }
            }
        }
    }

    private func searchResultView(_ result: SearchResult) -> some View {
        VStack(spacing: 16) {
            Spacer()

            Image(systemName: "sparkles")
                .font(.system(size: 32))
                .foregroundStyle(Color.accentColor)

            Text(result.context)
                .font(.subheadline)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .padding(.horizontal)

            Button {
                audioService.seek(to: result.timestamp)
                searchText = ""
                searchResult = nil
            } label: {
                HStack {
                    Image(systemName: "play.fill")
                    Text("Jump to \(formatTime(result.timestamp))")
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
            }
            .buttonStyle(.borderedProminent)

            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    private func performSearch() {
        guard !searchText.isEmpty else { return }
        guard let transcript = episode.transcript else { return }

        // If it looks like a question/command, use AI search
        let isQuestion = searchText.contains("?") ||
            searchText.lowercased().hasPrefix("where") ||
            searchText.lowercased().hasPrefix("when") ||
            searchText.lowercased().hasPrefix("take me") ||
            searchText.lowercased().hasPrefix("find") ||
            searchText.lowercased().hasPrefix("go to")

        guard isQuestion else { return }

        isSearching = true

        Task {
            let sortedChunks = transcript.chunks.sorted { $0.startTime < $1.startTime }
            let chunkData = sortedChunks.map { (text: $0.text, startTime: $0.startTime, endTime: $0.endTime) }

            if let result = try? await aiService.findRelevantTimestamp(query: searchText, chunks: chunkData) {
                await MainActor.run {
                    searchResult = SearchResult(timestamp: result.timestamp, context: result.context)
                    isSearching = false
                }
            } else {
                await MainActor.run {
                    isSearching = false
                }
            }
        }
    }

    private func formatTime(_ time: TimeInterval) -> String {
        let totalSeconds = Int(time)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60

        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        } else {
            return String(format: "%d:%02d", minutes, seconds)
        }
    }
}

private struct SearchResult {
    let timestamp: TimeInterval
    let context: String
}

struct ChapterRow: View {
    let chapter: Chapter
    let isActive: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(alignment: .top, spacing: 12) {
                Text(formatTime(chapter.startTime))
                    .font(.caption)
                    .fontWeight(.medium)
                    .foregroundStyle(isActive ? Color.accentColor : .secondary)
                    .frame(width: 44, alignment: .trailing)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(chapter.title)
                            .font(.subheadline)
                            .fontWeight(isActive ? .semibold : .medium)
                            .foregroundStyle(isActive ? .primary : .primary)

                        if chapter.isAd {
                            Text("AD")
                                .font(.caption2)
                                .fontWeight(.bold)
                                .foregroundStyle(.white)
                                .padding(.horizontal, 5)
                                .padding(.vertical, 2)
                                .background(Color.orange)
                                .clipShape(Capsule())
                        }
                    }

                    Text(chapter.summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Spacer()

                if isActive {
                    Image(systemName: "speaker.wave.2.fill")
                        .font(.caption)
                        .foregroundStyle(Color.accentColor)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(isActive ? Color.accentColor.opacity(0.1) : Color.clear)
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
