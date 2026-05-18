import SwiftUI

// MARK: - DownloadsManagerView

struct DownloadsManagerView: View {
    @Environment(AppStateStore.self) private var store
    @State private var downloadService = EpisodeDownloadService.shared
    @State private var confirmCancelActive = false
    @State private var confirmDeleteDownloaded = false

    var body: some View {
        List {
            summarySection

            if downloadRows.isEmpty {
                emptySection
            } else {
                if !activeRows.isEmpty {
                    activeSection
                }
                if !failedRows.isEmpty {
                    failedSection
                }
                if !downloadedRows.isEmpty {
                    downloadedSection
                }
                actionsSection
            }
        }
        .settingsListStyle()
        .navigationTitle("Downloads")
        .navigationBarTitleDisplayMode(.large)
        .task {
            downloadService.attach(appStore: store)
        }
        .alert("Cancel active downloads?", isPresented: $confirmCancelActive) {
            Button("Keep Downloads", role: .cancel) {}
            Button("Cancel Downloads", role: .destructive) {
                cancelActiveDownloads()
            }
        } message: {
            Text("This stops \(countLabel(activeRows.count, singular: "download")) currently downloading or queued episode\(activeRows.count == 1 ? "" : "s").")
        }
        .alert("Delete downloaded episodes?", isPresented: $confirmDeleteDownloaded) {
            Button("Keep Downloads", role: .cancel) {}
            Button("Delete Downloads", role: .destructive) {
                deleteDownloadedEpisodes()
            }
        } message: {
            Text("This removes \(countLabel(downloadedRows.count, singular: "downloaded episode")) from this device. Your library and playback progress are kept.")
        }
    }

    // MARK: - Sections

    private var summarySection: some View {
        Section {
            HStack(spacing: 0) {
                DownloadsSummaryStat(
                    value: activeRows.count,
                    label: "Active",
                    tint: .blue
                )
                Divider().padding(.vertical, 4)
                DownloadsSummaryStat(
                    value: failedRows.count,
                    label: "Failed",
                    tint: .orange
                )
                Divider().padding(.vertical, 4)
                DownloadsSummaryStat(
                    value: downloadedRows.count,
                    label: "Saved",
                    tint: .green
                )
            }
            .frame(minHeight: 58)
        } footer: {
            Text("Background downloads continue when the app leaves the foreground. Use this screen to inspect active work, retry failures, or free downloaded files.")
        }
    }

    private var emptySection: some View {
        Section {
            ContentUnavailableView(
                "No Downloads",
                systemImage: "arrow.down.circle",
                description: Text("Download an episode from any episode row or detail screen to see it here.")
            )
            .frame(maxWidth: .infinity)
            .padding(.vertical, AppTheme.Spacing.lg)
        }
    }

    private var activeSection: some View {
        Section("Active & Queued") {
            ForEach(activeRows) { row in
                DownloadsManagerRow(row: row, onAction: perform)
            }
        }
    }

    private var failedSection: some View {
        Section("Failed") {
            ForEach(failedRows) { row in
                DownloadsManagerRow(row: row, onAction: perform)
            }
        }
    }

    private var downloadedSection: some View {
        Section("Downloaded") {
            ForEach(downloadedRows) { row in
                DownloadsManagerRow(row: row, onAction: perform)
            }
        }
    }

    @ViewBuilder
    private var actionsSection: some View {
        if !activeRows.isEmpty || !downloadedRows.isEmpty {
            Section("Bulk Actions") {
                if !activeRows.isEmpty {
                    Button(role: .destructive) {
                        Haptics.warning()
                        confirmCancelActive = true
                    } label: {
                        Label("Cancel Active Downloads", systemImage: "xmark.circle")
                    }
                }
                if !downloadedRows.isEmpty {
                    Button(role: .destructive) {
                        Haptics.warning()
                        confirmDeleteDownloaded = true
                    } label: {
                        Label("Delete Downloaded Episodes", systemImage: "trash")
                    }
                }
            }
        }
    }

    // MARK: - Rows

    private var downloadRows: [DownloadManagerRowData] {
        let podcasts = Dictionary(uniqueKeysWithValues: store.state.podcasts.map { ($0.id, $0) })

        return store.state.episodes.compactMap { episode in
            guard let status = status(for: episode) else { return nil }
            let podcast = podcasts[episode.podcastID]
            return DownloadManagerRowData(
                episode: episode,
                showTitle: podcast?.title ?? "Unknown show",
                showAccent: podcast?.accentColor ?? .blue,
                artworkURL: episode.imageURL ?? podcast?.imageURL,
                status: status
            )
        }
    }

    private var activeRows: [DownloadManagerRowData] {
        downloadRows
            .filter(\.status.isActive)
            .sorted { lhs, rhs in
                if lhs.status.sortRank != rhs.status.sortRank {
                    return lhs.status.sortRank < rhs.status.sortRank
                }
                return lhs.episode.pubDate > rhs.episode.pubDate
            }
    }

    private var failedRows: [DownloadManagerRowData] {
        downloadRows
            .filter(\.status.isFailed)
            .sorted { $0.episode.pubDate > $1.episode.pubDate }
    }

    private var downloadedRows: [DownloadManagerRowData] {
        downloadRows
            .filter(\.status.isDownloaded)
            .sorted { $0.episode.pubDate > $1.episode.pubDate }
    }

    private func status(for episode: Episode) -> DownloadManagerStatus? {
        switch episode.downloadState {
        case .notDownloaded:
            return nil
        case .queued:
            return .queued
        case .downloading(let storedProgress, let bytesWritten):
            return .downloading(
                progress: (downloadService.progress[episode.id] ?? storedProgress).clampedDownloadProgress,
                bytesWritten: bytesWritten,
                expectedBytes: downloadService.expectedBytes[episode.id]
            )
        case .downloaded(_, let byteCount):
            return .downloaded(byteCount: byteCount)
        case .failed(let message):
            return .failed(message: message)
        }
    }

    // MARK: - Actions

    private func perform(_ action: DownloadManagerAction, row: DownloadManagerRowData) {
        downloadService.attach(appStore: store)
        switch action {
        case .start, .retry:
            Haptics.light()
            downloadService.download(episodeID: row.id)
        case .cancel:
            Haptics.light()
            if row.status.isQueued {
                store.setEpisodeDownloadState(row.id, state: .notDownloaded)
            } else {
                downloadService.cancel(episodeID: row.id)
            }
        case .clearFailed:
            Haptics.light()
            store.setEpisodeDownloadState(row.id, state: .notDownloaded)
        case .delete:
            Haptics.warning()
            downloadService.delete(episodeID: row.id)
        }
    }

    private func cancelActiveDownloads() {
        downloadService.attach(appStore: store)
        for row in activeRows {
            if row.status.isQueued {
                store.setEpisodeDownloadState(row.id, state: .notDownloaded)
            } else {
                downloadService.cancel(episodeID: row.id)
            }
        }
    }

    private func deleteDownloadedEpisodes() {
        downloadService.attach(appStore: store)
        for row in downloadedRows {
            downloadService.delete(episodeID: row.id)
        }
    }

    private func countLabel(_ count: Int, singular: String) -> String {
        count == 1 ? "1 \(singular)" : "\(count) \(singular)s"
    }
}
