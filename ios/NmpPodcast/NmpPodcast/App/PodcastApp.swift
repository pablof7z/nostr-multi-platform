import SwiftUI

/// The top-level entry point for the app. Sets up global environment objects
/// and wires the Nostr relay service to relevant settings changes.
@main
struct PodcastApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @State private var store = AppStateStore()
    @State private var userIdentity = UserIdentityStore.shared
    @State private var relayService: NostrRelayService?
    @State private var scheduledTaskRunner: AgentScheduledTaskRunner?
    /// T-podcast-ios-RESTART: Kernel bridge injected alongside AppStateStore so
    /// Library tab can read real podcast data from the Rust kernel. This is the
    /// only addition vs Podcastr's AppMain.swift — all other lines are identical.
    @StateObject private var kernelModel = KernelModel()
    /// Single global owner-consultation coordinator. Lives here (not on
    /// `AgentChatSession`) so an inbound peer-agent reply flowing through
    /// `AgentRelayBridge` can pop the same sheet even when the user is on
    /// Home / Library / Wiki — i.e. while no chat session exists. Mounted
    /// on `RootView` via `agentAskPresenter(coordinator:)`.
    @State private var askCoordinator = AgentAskCoordinator()

    // MARK: - What's-new sheet wiring
    //
    // Evaluated once on cold launch (`.task` below). Stays here in
    // `AppMain.swift` rather than `RootView.swift` so the "what changed
    // since you last opened the app" check fires before any tab-level
    // view has a chance to short-circuit it.
    //
    // Single optional `@State` + `.sheet(item:)` rather than the more
    // common pair of `entries: [...]` and `isPresented: Bool`. The
    // `OnboardingView` fullScreenCover sits on top of RootView during
    // first launch, and SwiftUI re-evaluates the queued sheet's content
    // closure once the cover dismisses. With the two-state pattern the
    // closure was reading a stale `entries = []` from across that
    // render boundary, so the sheet rendered empty. `.sheet(item:)`
    // passes the entries through the trigger itself, eliminating the
    // race.
    @State private var whatsNewPresentation: WhatsNewPresentation?

    var body: some Scene {
        WindowGroup {
            RootView(relayService: relayService, scheduledTaskRunner: scheduledTaskRunner)
                .environment(store)
                .environment(userIdentity)
                .environment(askCoordinator)
                .environmentObject(kernelModel)
                .task { CarPlayController.shared.attach(store: store) }
                .task { kernelModel.start() }
                // T-podcast-ios-3: bind AppStateStore to KernelModel so that
                // store.allPodcasts / subscription / deletePodcast proxy through
                // the kernel snapshot. Verbatim Podcastr views read `store`; the
                // kernel owns the truth.
                .task { store.bind(kernelModel: kernelModel) }
                .task {
                    // Order matters: NDK must exist before UserIdentityStore's
                    // generated-profile self-publish path reaches the signer
                    // shim, which routes through NDKEventBuilder.
                    await NostrStack.shared.bind(store: store)
                    userIdentity.start()
                    await NostrStack.shared.start()
                    let service = NostrRelayService(store: store, askCoordinator: askCoordinator)
                    relayService = service
                    service.start()
                    scheduledTaskRunner = AgentScheduledTaskRunner(store: store)
                }
                .task {
                    // Seed a fresh install silently so the first launch
                    // doesn't dump the entire changelog as "new."
                    WhatsNewService.seedIfNeeded()
                    let unseen = WhatsNewService.unseenEntries(
                        lastSeenAt: WhatsNewService.lastSeenAt
                    )
                    if !unseen.isEmpty {
                        whatsNewPresentation = WhatsNewPresentation(entries: unseen)
                    }
                }
                .task {
                    // One-shot backfill: ensure every episode the user
                    // already has in the library has its title +
                    // description embedded so `find_similar_episodes` /
                    // `search_episodes` can surface untranscribed
                    // episodes. Throttled + reentrancy-safe; subsequent
                    // launches no-op once everything is flagged.
                    await EpisodeMetadataIndexer.shared.runBackfill(appStore: store)
                }
                .sheet(item: $whatsNewPresentation) { presentation in
                    WhatsNewSheet(entries: presentation.entries)
                }
                .onChange(of: store.state.settings.nostrEnabled) { _, _ in
                    Task { await NostrStack.shared.start() }
                    relayService?.start()
                }
                .onChange(of: store.state.settings.nostrRelayURL) { _, _ in
                    Task { await NostrStack.shared.start() }
                    relayService?.start()
                }
                .onChange(of: store.state.settings.nostrPublicKeyHex) { _, _ in relayService?.start() }
                .onChange(of: store.state.settings.nostrProfileName) { _, _ in relayService?.republishProfile() }
                .onChange(of: store.state.settings.nostrProfileAbout) { _, _ in relayService?.republishProfile() }
                .onChange(of: store.state.settings.nostrProfilePicture) { _, _ in relayService?.republishProfile() }
        }
    }
}

/// Drives the What's New `.sheet(item:)`. Bundling the entries with the
/// trigger guarantees the sheet content closure receives them atomically
/// — see the wiring note above.
private struct WhatsNewPresentation: Identifiable {
    let id = UUID()
    let entries: [WhatsNewEntry]
}
