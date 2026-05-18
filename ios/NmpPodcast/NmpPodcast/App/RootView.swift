import CoreSpotlight
// import ShakeFeedbackKit  // T-podcast-ios-RESTART: shim provided in Bridge/PodcastrShims.swift
import SwiftUI

/// The tabs available at the root navigation level.
///
/// Search is reachable via a top-right toolbar button. The Player lives behind
/// a persistent mini-bar that expands into `PlayerView` on tap. Settings,
/// Clippings, and Wiki are reachable from the avatar sidebar.
enum RootTab: String, CaseIterable {
    case home = "Home"
    case library = "Library"
    case bookmarks = "Bookmarks"
    case clippings = "Clippings"
    case wiki = "Wiki"

    var icon: String {
        switch self {
        case .home:      "house.fill"
        case .library:   "tray.fill"
        case .bookmarks: "bookmark.fill"
        case .clippings: "scissors"
        case .wiki:      "book.closed.fill"
        }
    }
}

/// The root view of the app. Hosts the main tab bar (hidden), the feedback
/// shake gesture, onboarding gate, deep-link routing, and the avatar sidebar.
struct RootView: View {
    let relayService: NostrRelayService?
    let scheduledTaskRunner: AgentScheduledTaskRunner?

    @Environment(AppStateStore.self) var store
    @Environment(AgentAskCoordinator.self) var askCoordinator
    @Environment(UserIdentityStore.self) var userIdentity
    @State var selectedTab: RootTab = .home
    @State var feedbackWorkflow = FeedbackWorkflow()
    @State var sharedFeedbackStore = ShakeFeedbackStore(config: .podcastr, namespace: "io.f7z.podcast")
    @State var showFeedback = false
    @State var showSettings = false
    @State var showAgentChat = false
    @State var showSidebar = false
    @State var showSearch = false
    @State var agentSession: AgentChatSession?
    @State var agentUnseenMessageCount: Int = 0
    @State var showVoiceMode = false
    @State var lastShakeTime: Date = .distantPast
    @State var spotlightSheet: SpotlightIndexer.DeepLink?
    @State var playbackState = PlaybackState()
    @State var showFullPlayer = false
    @State var playerNavSubscriptionID: UUID?
    @State var generationSourceNostrRootID: String?
    @Namespace var playerNamespace

    var relayServiceIdentity: ObjectIdentifier? {
        relayService.map(ObjectIdentifier.init)
    }

    private let sidebarWidth: CGFloat = 300

    var body: some View {
        ZStack(alignment: .leading) {
            tabBar
                .environment(playbackState)
                .offset(x: showSidebar ? sidebarWidth : 0)
                .overlay {
                    if showSidebar {
                        Color.clear
                            .ignoresSafeArea()
                            .contentShape(Rectangle())
                            .onTapGesture {
                                Haptics.selection()
                                withAnimation(AppTheme.Animation.spring) { showSidebar = false }
                            }
                    }
                }
                .task(id: relayServiceIdentity) {
                    guard let relayService else { return }
                    relayService.agentResponder.podcastDepsProvider = { [store, playbackState] in
                        LivePodcastAgentToolDeps.make(store: store, playback: playbackState)
                    }
                    relayService.agentResponder.askCoordinator = askCoordinator
                    scheduledTaskRunner?.podcastDepsProvider = { [store, playbackState] in
                        LivePodcastAgentToolDeps.make(store: store, playback: playbackState)
                    }
                    scheduledTaskRunner?.runDueTasksIfNeeded()
                }
                .onAppear { setupPlaybackHandlers() }
                .onChange(of: store.state.settings) { _, new in
                    playbackState.autoMarkPlayedOnFinish = new.autoMarkPlayedAtEnd
                    playbackState.applyPreferences(from: new)
                }
                .sheet(isPresented: $showFullPlayer) {
                    PlayerView(state: playbackState, glassNamespace: playerNamespace)
                        .presentationDetents([.large])
                        .presentationDragIndicator(.visible)
                        .presentationBackgroundInteraction(.disabled)
                }
                .onShake { handleShake() }
                .sheet(isPresented: $showFeedback) {
                    ShakeFeedbackSheet(store: sharedFeedbackStore)
                        .presentationDetents([.large])
                }
                .task(id: userIdentity.publicKeyHex) {
                    guard userIdentity.publicKeyHex != nil else { return }
                    await sharedFeedbackStore.start(hostSigner: PodcastShakeFeedbackSigner(identity: userIdentity))
                }
                .sheet(isPresented: $showSettings) {
                    NavigationStack { SettingsView() }
                }
                .sheet(isPresented: $showAgentChat) {
                    if let session = agentSession {
                        NavigationStack {
                            AgentChatView(session: session)
                                .toolbar {
                                    ToolbarItem(placement: .topBarLeading) {
                                        Button("Done") {
                                            Haptics.selection()
                                            showAgentChat = false
                                        }
                                    }
                                }
                        }
                        .environment(playbackState)
                    }
                }
                .onChange(of: showAgentChat) { _, _ in
                    agentUnseenMessageCount = agentSession?.messages.count ?? 0
                }
                .sheet(item: Binding(
                    get: { spotlightSheet.map(IdentifiedSpotlightLink.init) },
                    set: { spotlightSheet = $0?.link }
                )) { identified in
                    NavigationStack { spotlightDetailView(for: identified.link) }
                }
                .fullScreenCover(
                    isPresented: .init(
                        get: { feedbackWorkflow.isAnnotationVisible },
                        set: { if !$0 { feedbackWorkflow.phase = .composing } }
                    )
                ) {
                    ScreenshotAnnotationView(workflow: feedbackWorkflow)
                }
                .fullScreenCover(
                    isPresented: Binding(
                        get: { !store.state.settings.hasCompletedOnboarding },
                        set: { _ in }
                    )
                ) {
                    OnboardingView()
                }
                .fullScreenCover(isPresented: $showVoiceMode) {
                    VoiceView(onSwitchToText: {
                        showVoiceMode = false
                        openAgentChat()
                    })
                }
                .sheet(isPresented: $showSearch) { searchSheet }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.willEnterForegroundNotification)) { _ in
                    scheduledTaskRunner?.runDueTasksIfNeeded()
                }
                .onReceive(NotificationCenter.default.publisher(for: .voiceModeRequested)) { _ in
                    showVoiceMode = true
                }
                .onReceive(NotificationCenter.default.publisher(for: .askAgentRequested)) { _ in
                    showFullPlayer = false
                    openAgentChat()
                }
                .onReceive(NotificationCenter.default.publisher(for: .openPlayerRequested)) { _ in
                    showFullPlayer = true
                }
                .onReceive(NotificationCenter.default.publisher(for: .openSubscriptionDetailRequested)) { note in
                    guard let idString = note.userInfo?["subscriptionID"] as? String,
                          let uuid = UUID(uuidString: idString) else { return }
                    showFullPlayer = false
                    playerNavSubscriptionID = uuid
                }
                .onReceive(NotificationCenter.default.publisher(for: .openAgentChatConversation)) { note in
                    guard let convID = note.userInfo?["conversationID"] as? UUID else { return }
                    showFullPlayer = false
                    if agentSession == nil {
                        agentSession = AgentChatSession(
                            store: store,
                            playback: playbackState,
                            askCoordinator: askCoordinator
                        )
                    }
                    Task { @MainActor in
                        await agentSession?.switchToConversation(convID)
                        showAgentChat = true
                    }
                }
                .onReceive(NotificationCenter.default.publisher(for: .openNostrConversationRequested)) { note in
                    guard let rootID = note.userInfo?["rootEventID"] as? String else { return }
                    showFullPlayer = false
                    generationSourceNostrRootID = rootID
                }
                .modifier(PlayerNavSheets(
                    subscriptionID: $playerNavSubscriptionID,
                    store: store
                ))
                .sheet(item: Binding(
                    get: { generationSourceNostrRootID.map(IdentifiedString.init) },
                    set: { generationSourceNostrRootID = $0?.value }
                )) { identified in
                    if let convo = store.state.nostrConversations.first(where: { $0.rootEventID == identified.value }) {
                        NavigationStack {
                            NostrConversationDetailView(conversation: convo)
                                .toolbar {
                                    ToolbarItem(placement: .topBarLeading) {
                                        Button("Done") { generationSourceNostrRootID = nil }
                                    }
                                }
                        }
                    }
                }
                .nostrApprovalPresenter()
                .nostrAgentSurface()
                .agentAskPresenter(coordinator: askCoordinator)
                .onOpenURL { handleDeepLink($0) }
                .onReceive(
                    NotificationCenter.default.publisher(for: AppDelegate.shortcutURLNotification)
                ) { note in
                    if let url = note.object as? URL { handleDeepLink(url) }
                }
                .onContinueUserActivity(CSSearchableItemActionType, perform: handleSpotlight)

            AppSidebarView(
                selectedTab: $selectedTab,
                isPresented: $showSidebar
            )
            .frame(width: sidebarWidth)
            .ignoresSafeArea()
            .offset(x: showSidebar ? 0 : -sidebarWidth)
            .zIndex(100)
        }
    }

    // MARK: - Tab bar

    @ViewBuilder
    private var tabBar: some View {
        let base = TabView(selection: $selectedTab) {
            ForEach(RootTab.allCases, id: \.self) { tab in
                tabContent(for: tab)
                    .tabItem { Label(tab.rawValue, systemImage: tab.icon) }
                    .tag(tab)
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)

        if playbackState.episode != nil {
            base.tabViewBottomAccessory {
                MiniPlayerView(
                    state: playbackState,
                    onTap: { showFullPlayer = true },
                    glassNamespace: playerNamespace
                )
            }
        } else {
            base
        }
    }

    @ViewBuilder
    private func tabContent(for tab: RootTab) -> some View {
        switch tab {
        case .home:
            NavigationStack {
                HomeView()
                    .toolbar { sharedToolbar() }
            }
            .toolbar(.hidden, for: .tabBar)
        case .library:
            NavigationStack {
                AllEpisodesView()
                    .toolbar { sharedToolbar() }
            }
            .toolbar(.hidden, for: .tabBar)
        case .bookmarks:
            NavigationStack {
                BookmarksView()
                    .toolbar { sharedToolbar() }
            }
            .toolbar(.hidden, for: .tabBar)
        case .clippings:
            NavigationStack {
                ClippingsView()
                    .toolbar { sharedToolbar() }
            }
            .toolbar(.hidden, for: .tabBar)
        case .wiki:
            NavigationStack {
                WikiView()
                    .toolbar { sharedToolbar() }
            }
            .toolbar(.hidden, for: .tabBar)
        }
    }

    // MARK: - Search sheet

    private var searchSheet: some View {
        NavigationStack {
            PodcastSearchView()
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("Done") {
                            Haptics.selection()
                            showSearch = false
                        }
                    }
                }
        }
        .environment(playbackState)
    }

    // MARK: - Helpers

    var hasUnreadAgentMessages: Bool {
        guard !showAgentChat, let session = agentSession else { return false }
        return session.messages.count > agentUnseenMessageCount
    }

    func openAgentChat() {
        if agentSession == nil {
            agentSession = AgentChatSession(
                store: store,
                playback: playbackState,
                askCoordinator: askCoordinator
            )
        }
        showAgentChat = true
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder
    private func sharedToolbar() -> some ToolbarContent {
        ToolbarItem(placement: .topBarLeading) {
            let profile = UserProfileDisplay.from(identity: userIdentity)
            Button {
                Haptics.selection()
                withAnimation(AppTheme.Animation.spring) { showSidebar = true }
            } label: {
                IdentityAvatarView(
                    url: profile?.pictureURL,
                    initial: profile?.displayName.first,
                    size: 28
                )
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open sidebar")
        }
        ToolbarItem(placement: .topBarLeading) {
            Button {
                Haptics.selection()
                showSettings = true
            } label: {
                Image(systemName: "gear")
            }
            .accessibilityLabel("Settings")
        }
        NostrConversationsToolbarItem()
        ToolbarItem(placement: .topBarTrailing) {
            Button {
                Haptics.selection()
                showSearch = true
            } label: {
                Image(systemName: "magnifyingglass")
            }
            .accessibilityLabel("Search")
        }
        ToolbarItem(placement: .topBarTrailing) {
            Button {
                Haptics.selection()
                openAgentChat()
            } label: {
                Image(systemName: "sparkles")
                    .overlay(alignment: .topTrailing) {
                        if hasUnreadAgentMessages {
                            Circle()
                                .fill(.red)
                                .frame(width: 7, height: 7)
                                .offset(x: 4, y: -4)
                                .transition(.scale.combined(with: .opacity))
                        }
                    }
                    .animation(AppTheme.Animation.springFast, value: hasUnreadAgentMessages)
            }
            .accessibilityLabel(hasUnreadAgentMessages ? "Open Agent — new reply" : "Open Agent")
            .keyboardShortcut("a", modifiers: [.command, .shift])
        }
    }

    // MARK: - Helper types

    private struct IdentifiedString: Identifiable {
        let value: String
        var id: String { value }
    }

    private struct IdentifiedSpotlightLink: Identifiable {
        let link: SpotlightIndexer.DeepLink

        var id: String {
            switch link {
            case .note(let id):         return "note:\(id)"
            case .memory(let id):       return "memory:\(id)"
            case .subscription(let id): return "subscription:\(id)"
            case .episode(let id):      return "episode:\(id)"
            }
        }
    }
}
