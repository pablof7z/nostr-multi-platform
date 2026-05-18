import SwiftUI

struct SettingsView: View {
    @Environment(AppStateStore.self) private var store
    @State private var storageSummary: String?

    var body: some View {
        List {
            accountSection
            librarySection
            listeningSection
            intelligenceSection
            systemSection
            versionFooterSection
        }
        .settingsListStyle()
        .navigationTitle("Settings")
        .navigationBarTitleDisplayMode(.large)
        .task {
            // Cheap directory walk; runs once when Settings opens so the
            // Data & Storage row can show the total without a navigation push.
            let snap = await StorageSettingsView.compute(store: store)
            await MainActor.run {
                storageSummary = snap.totalBytes > 0 ? Self.formatSize(snap.totalBytes) : nil
            }
        }
    }

    // MARK: - Sections

    private var accountSection: some View {
        Section("Account") {
            IdentitySettingsRow()
        }
    }

    private var librarySection: some View {
        Section("Library") {
            NavigationLink {
                SubscriptionsListView()
            } label: {
                SettingsRow(
                    icon: "antenna.radiowaves.left.and.right",
                    tint: .pink,
                    title: "Subscriptions",
                    value: subscriptionCountLabel
                )
            }
            NavigationLink {
                CategoriesListView()
            } label: {
                SettingsRow(
                    icon: "square.grid.2x2.fill",
                    tint: .mint,
                    title: "Categories",
                    value: categoryCountLabel
                )
            }
        }
    }

    private var listeningSection: some View {
        Section("Listening") {
            NavigationLink {
                PlaybackSettingsView()
            } label: {
                SettingsRow(
                    icon: "play.rectangle.fill",
                    tint: .blue,
                    title: "Player",
                    subtitle: playbackSummary
                )
            }

            NavigationLink {
                DownloadsManagerView()
            } label: {
                SettingsRow(
                    icon: "arrow.down.circle.fill",
                    tint: .blue,
                    title: "Downloads",
                    value: downloadsSummaryLabel
                )
            }
        }
    }

    private var intelligenceSection: some View {
        Section("Intelligence") {
            NavigationLink {
                AgentSettingsView()
            } label: {
                SettingsRow(
                    icon: "brain.head.profile",
                    tint: .orange,
                    title: "Agent",
                    badge: store.pendingNostrApprovals.count
                )
            }

            NavigationLink {
                AIProvidersSettingsView()
            } label: {
                SettingsRow(
                    icon: "key.viewfinder",
                    tint: .indigo,
                    title: "Providers",
                    value: providersRowValue
                )
            }

            NavigationLink {
                AIModelsSettingsView()
            } label: {
                SettingsRow(
                    icon: "slider.horizontal.3",
                    tint: .purple,
                    title: "Models",
                    value: currentModelShortName
                )
            }

            NavigationLink {
                TranscriptsSettingsView()
            } label: {
                SettingsRow(
                    icon: "captions.bubble.fill",
                    tint: .orange,
                    title: "Transcripts",
                    value: transcriptStatus
                )
            }

            NavigationLink {
                WikiSettingsView()
            } label: {
                SettingsRow(
                    icon: "book.closed.fill",
                    tint: .indigo,
                    title: "Wiki",
                    value: wikiRowValue
                )
            }
        }
    }

    private var systemSection: some View {
        SettingsSystemSection(
            notificationsRowValue: notificationsRowValue,
            networkingRowValue: networkingRowValue,
            dataStorageSummary: dataStorageSummary
        )
    }

    static func formatSize(_ bytes: Int64) -> String {
        bytes.formattedFileSize
    }

    private var versionFooterSection: some View {
        Section {
        } footer: {
            Text(appVersionFooter)
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.tertiary)
                .frame(maxWidth: .infinity, alignment: .center)
        }
    }

    // MARK: - Derived values

    private var dataRecordCount: Int {
        store.state.subscriptions.count
            + store.state.episodes.count
            + store.activeNotes.count
            + store.activeMemories.count
            + store.state.friends.count
            + store.activeAgentActivityCount
    }

    private var currentModelShortName: String {
        Settings.modelDisplayName(
            modelID: store.state.settings.agentInitialModel,
            modelName: store.state.settings.agentInitialModelName
        )
    }

    private var subscriptionCountLabel: String? {
        let count = store.state.subscriptions.count
        guard count > 0 else { return nil }
        return "\(count)"
    }

    private var categoryCountLabel: String? {
        let count = store.state.categories.count
        guard count > 0 else { return nil }
        return "\(count)"
    }

    private var downloadsSummaryLabel: String? {
        var active = 0
        var failed = 0
        var downloaded = 0
        for episode in store.state.episodes {
            switch episode.downloadState {
            case .queued, .downloading:
                active += 1
            case .failed:
                failed += 1
            case .downloaded:
                downloaded += 1
            case .notDownloaded:
                break
            }
        }
        if active > 0 { return "\(active) active" }
        if failed > 0 { return "\(failed) failed" }
        if downloaded > 0 { return "\(downloaded) saved" }
        return nil
    }

    private var playbackSummary: String {
        let s = store.state.settings
        let rate: String
        if abs(s.defaultPlaybackRate - 1.0) < 0.001 {
            rate = "1×"
        } else {
            rate = String(format: "%.1f×", s.defaultPlaybackRate)
        }
        return "\(rate) · \(s.skipBackwardSeconds)s back · \(s.skipForwardSeconds)s forward"
    }

    private var transcriptStatus: String {
        let s = store.state.settings
        let provider = s.sttProvider.displayName
        switch (s.autoIngestPublisherTranscripts, s.autoFallbackToScribe) {
        case (true, true):  return "Auto + \(provider)"
        case (true, false): return "Auto only"
        case (false, true): return "\(provider) fallback"
        case (false, false): return "Manual"
        }
    }

    private var notificationsRowValue: String? {
        let s = store.state.settings
        var on: [String] = []
        if s.notifyOnNewEpisodes  { on.append("Episodes") }
        if s.notifyOnBriefingReady { on.append("Briefings") }
        if on.isEmpty { return "Off" }
        if on.count == 2 { return "On" }
        return on.first
    }

    private var providersRowValue: String {
        let s = store.state.settings
        let connected = [
            s.openRouterCredentialSource != .none,
            s.elevenLabsCredentialSource != .none,
            s.ollamaCredentialSource != .none,
        ].filter { $0 }.count
        return connected == 0 ? "Not set up" : "\(connected) connected"
    }

    private var wikiRowValue: String {
        "Manual"
    }

    private var networkingRowValue: String {
        NostrStack.shared.relaysConnected ? "Connected" : "Offline"
    }

    private var dataStorageSummary: String? {
        if let storageSummary { return storageSummary }
        guard dataRecordCount > 0 else { return nil }
        return "\(dataRecordCount) records"
    }

    private var appVersionFooter: String { Self.cachedVersionFooter }

    private static let cachedVersionFooter: String = {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.0"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "1"
        return "Podcastr  \(version)  (build \(build))"
    }()
}
