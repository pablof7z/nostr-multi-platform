import SwiftUI

// MARK: - ShowDetailView

/// Show-level detail screen pushed from `LibraryView`'s grid.
///
/// **Composition:**
///   - Bleed-edge tint gradient that extends past the safe area / nav bar.
///   - `ShowDetailHeader` — artwork-on-left + title/description on right.
///   - "Episodes" section header.
///   - Episode list — `EpisodeRow` × N, tapping pushes
///     `LibraryEpisodeRoute` onto the enclosing `NavigationStack`.
///
/// **Glass usage:** none on the body. The "Settings for this show" sheet
/// (presented from the toolbar `…` menu) is structurally glass.
struct ShowDetailView: View {

    @Environment(AppStateStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    let podcast: Podcast

    @State private var showSettings: Bool = false
    @State private var showUnsubscribeConfirm: Bool = false
    @State private var showDeleteConfirm: Bool = false
    @State private var showDownloadAllConfirm: Bool = false
    @State private var searchText: String = ""
    @State private var isSearchActive: Bool = false
    @State private var isFetchingEpisodes: Bool = false
    /// Drives the VoiceOver "Open episode details" custom action — bound into
    /// `ShowDetailEpisodeList` and consumed via `.navigationDestination(item:)`
    /// so the same `EpisodeDetailView` opens regardless of how the user got there.
    @State private var voiceOverDetailRoute: LibraryEpisodeRoute?

    var body: some View {
        List {
            Section {
                ShowDetailHeader(
                    podcast: liveSubscription,
                    episodeCount: episodes.count
                )
                .listRowInsets(EdgeInsets())
                .listRowSeparator(.hidden)
                .listRowBackground(Color.clear)

                episodesHeader
                    .listRowInsets(EdgeInsets(
                        top: AppTheme.Spacing.lg,
                        leading: AppTheme.Spacing.lg,
                        bottom: AppTheme.Spacing.sm,
                        trailing: AppTheme.Spacing.lg
                    ))
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
            }

            episodeListSection
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background {
            VStack(spacing: 0) {
                LinearGradient(
                    colors: [
                        liveSubscription.accentColor.opacity(0.55),
                        liveSubscription.accentColor.opacity(0.18),
                        Color(.systemBackground)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
                .frame(height: 320)
                Color(.systemBackground)
            }
            .ignoresSafeArea()
        }
        .navigationTitle(liveSubscription.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(.hidden, for: .navigationBar)
        .toolbar { toolbarContent }
        .searchable(
            text: $searchText,
            isPresented: $isSearchActive,
            placement: .navigationBarDrawer(displayMode: .always),
            prompt: "Search episodes"
        )
        .refreshable { await refresh() }
        .task(id: podcast.id) {
            guard !isFollowed, liveSubscription.feedURL != nil else { return }
            isFetchingEpisodes = true
            await refresh()
            isFetchingEpisodes = false
        }
        .sheet(isPresented: $showSettings) {
            ShowDetailSettingsSheet(
                podcast: liveSubscription,
                store: store,
                onDismiss: { showSettings = false },
                onUnsubscribe: { confirmUnsubscribe() }
            )
        }
        // `.alert` rather than `.confirmationDialog` because the dialog is
        // anchored to the toolbar's `Menu`. iOS 26 renders confirmationDialog
        // anchored to a menu as a popover and elides any `role: .cancel`
        // button (the popover's tap-outside-to-dismiss is treated as the
        // implicit cancel). That leaves the user staring at a single red
        // "Unsubscribe" button with no visible escape — a real UX trap for a
        // destructive action that wipes thousands of episodes. `.alert` is
        // a centred modal and reliably renders both buttons regardless of
        // anchor context.
        .alert(
            "Unsubscribe from \(liveSubscription.title)?",
            isPresented: $showUnsubscribeConfirm
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Unsubscribe", role: .destructive) { performUnsubscribe() }
        } message: {
            Text("This removes the show and all of its episodes from your library.")
        }
        .alert(
            "Delete \(liveSubscription.title)?",
            isPresented: $showDeleteConfirm
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { performUnsubscribe() }
        } message: {
            Text("This removes the podcast and every episode of it from your library. This cannot be undone.")
        }
        .alert(
            "Download All Episodes?",
            isPresented: $showDownloadAllConfirm
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Download \(notDownloadedCount)") { downloadAllEpisodes() }
        } message: {
            Text("This will download \(notDownloadedCount) episode\(notDownloadedCount == 1 ? "" : "s") (\(liveSubscription.title)). Transcripts will be generated automatically after each download.")
        }
        .navigationDestination(for: LibraryEpisodeRoute.self) { route in
            LibraryEpisodePlaceholder(route: route)
        }
        .navigationDestination(item: $voiceOverDetailRoute) { route in
            LibraryEpisodePlaceholder(route: route)
        }
    }

    // MARK: - Live snapshot

    /// Re-read the podcast from the store on every render so settings
    /// updates (notifications toggle, refresh metadata) are reflected.
    private var liveSubscription: Podcast {
        store.podcast(id: podcast.id) ?? podcast
    }

    private var episodes: [Episode] {
        store.episodes(forPodcast: podcast.id)
    }

    private var filteredEpisodes: [Episode] {
        guard !searchText.isEmpty else { return episodes }
        return episodes.filter {
            $0.title.localizedCaseInsensitiveContains(searchText) ||
            $0.description.localizedCaseInsensitiveContains(searchText)
        }
    }

    // MARK: - Pieces

    private var episodesHeader: some View {
        HStack {
            Text("Episodes")
                .font(AppTheme.Typography.title)
            if !searchText.isEmpty {
                Text("\(filteredEpisodes.count) of \(episodes.count)")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }

    @ViewBuilder
    private var episodeListSection: some View {
        if isFetchingEpisodes && episodes.isEmpty {
            Section {
                ProgressView("Loading episodes…")
                    .frame(maxWidth: .infinity)
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
                    .padding(.top, AppTheme.Spacing.xl)
            }
        } else if episodes.isEmpty {
            Section {
                ContentUnavailableView(
                    "No episodes yet",
                    systemImage: "tray",
                    description: Text("Pull down to refresh this feed.")
                )
                .listRowSeparator(.hidden)
                .listRowBackground(Color.clear)
                .padding(.top, AppTheme.Spacing.xl)
            }
        } else if filteredEpisodes.isEmpty {
            Section {
                ContentUnavailableView.search(text: searchText)
                    .listRowSeparator(.hidden)
                    .listRowBackground(Color.clear)
                    .padding(.top, AppTheme.Spacing.xl)
            }
        } else {
            Section {
                ShowDetailEpisodeList(
                    podcast: liveSubscription,
                    episodes: filteredEpisodes,
                    voiceOverDetailRoute: $voiceOverDetailRoute
                )
            }
        }
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Button {
                Haptics.light()
                isSearchActive = true
            } label: {
                Image(systemName: "magnifyingglass")
                    .font(.title3)
            }
            .accessibilityLabel("Search episodes")
        }
        ToolbarItem(placement: .topBarTrailing) {
            Menu {
                if isFollowed {
                    Button {
                        Haptics.light()
                        showSettings = true
                    } label: {
                        Label("Settings for this show", systemImage: "slider.horizontal.3")
                    }
                }
                if !episodes.isEmpty {
                    Button {
                        Haptics.light()
                        showDownloadAllConfirm = true
                    } label: {
                        Label("Download all episodes", systemImage: "arrow.down.circle")
                    }
                    .disabled(notDownloadedCount == 0)
                }
                if !isFollowed, liveSubscription.feedURL != nil {
                    // Unfollowed but has a real RSS feed — offer to follow.
                    // The "settings" surface is hidden until the user
                    // actually follows; toggles like notifications and
                    // auto-download have no subscription row to mutate yet.
                    Button {
                        Haptics.light()
                        Task { await follow() }
                    } label: {
                        Label("Follow", systemImage: "plus.circle")
                    }
                }
                // Share-show — recipients with podcast apps will recognize
                // the RSS URL and subscribe; everyone else gets a clickable
                // link with the show name above it via SharePreview.
                if let feedURL = liveSubscription.feedURL {
                    ShareLink(
                        item: feedURL,
                        preview: SharePreview(
                            sharePreviewTitle,
                            image: Image(systemName: "antenna.radiowaves.left.and.right")
                        )
                    ) {
                        Label("Share show", systemImage: "square.and.arrow.up")
                    }
                }
                if isFollowed {
                    Button(role: .destructive) {
                        Haptics.warning()
                        showUnsubscribeConfirm = true
                    } label: {
                        Label("Unsubscribe", systemImage: "minus.circle")
                    }
                } else {
                    // Unfollowed podcast — no "Unsubscribe" verb makes sense.
                    // The destructive option deletes the podcast row and all
                    // of its episodes (the All Podcasts swipe behaviour).
                    Button(role: .destructive) {
                        Haptics.warning()
                        showDeleteConfirm = true
                    } label: {
                        Label("Delete podcast", systemImage: "trash")
                    }
                }
            } label: {
                Image(systemName: "ellipsis.circle")
                    .font(.title3)
            }
            .accessibilityLabel("Show options")
        }
    }

    private var isFollowed: Bool {
        store.subscription(podcastID: podcast.id) != nil
    }

    private func follow() async {
        guard let feedURL = liveSubscription.feedURL else { return }
        do {
            try await SubscriptionService(store: store).addSubscription(feedURLString: feedURL.absoluteString)
            Haptics.success()
        } catch {
            Haptics.warning()
        }
    }

    /// "Show by Author" when the author is known, otherwise just the title.
    /// Used as the SharePreview header so destination apps see the show
    /// name + attribution rather than the raw RSS URL.
    private var sharePreviewTitle: String {
        let title = liveSubscription.title.isEmpty
            ? (liveSubscription.feedURL?.host ?? "Podcast")
            : liveSubscription.title
        return liveSubscription.author.isEmpty
            ? title
            : "\(title) by \(liveSubscription.author)"
    }

    // MARK: - Actions

    private func confirmUnsubscribe() {
        showUnsubscribeConfirm = true
    }

    private func performUnsubscribe() {
        store.deletePodcast(podcastID: podcast.id)
        dismiss()
    }

    private func refresh() async {
        await SubscriptionService(store: store).refresh(podcast)
    }

    /// Episodes that still need downloading (excludes in-flight and already downloaded).
    private var notDownloadedCount: Int {
        episodes.filter {
            switch $0.downloadState {
            case .downloaded, .downloading, .queued: return false
            default: return true
            }
        }.count
    }

    private func downloadAllEpisodes() {
        let service = EpisodeDownloadService.shared
        service.attach(appStore: store)
        for episode in episodes {
            switch episode.downloadState {
            case .downloaded, .downloading, .queued: continue
            default: service.download(episodeID: episode.id)
            }
        }
        Haptics.success()
    }
}
