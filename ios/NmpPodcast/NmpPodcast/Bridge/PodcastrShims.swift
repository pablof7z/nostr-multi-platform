import SwiftUI
import Combine
import CoreSpotlight
import os.log
import UserNotifications

// ─────────────────────────────────────────────────────────────────────────────
// PodcastrShims.swift
//
// Hollow shim types that mirror Podcastr's public API so that every View file
// copied verbatim from /Users/pablofernandez/Work/podcast/App/Sources can
// compile without modification.
//
// APPROACH A (preferred per T-podcast-ios-RESTART brief): the shims satisfy
// the Swift type-checker; runtime behavior is empty/no-op or backed by
// KernelModel where kernel data exists. No logic lives here — logic is in
// Rust (apps/podcast/).
//
// Each stub is documented with the gap task it blocks.
// ─────────────────────────────────────────────────────────────────────────────

// MARK: - DownloadState (verbatim match to Podcastr's DownloadState.swift)
//
// Top-level so Episode can typealias it and DownloadsManagerView patterns compile.

enum EpisodeDownloadState: Codable, Hashable {
    case notDownloaded
    case queued
    /// progress is 0...1; bytesWritten may be nil.
    case downloading(progress: Double, bytesWritten: Int64?)
    /// localFileURL and byteCount.
    case downloaded(localFileURL: URL, byteCount: Int64)
    case failed(message: String)
}

// MARK: - Domain models

/// T-podcast-gap-001: Full Episode model backed by kernel snapshot.
struct Episode: Identifiable, Hashable, Codable {
    let id: UUID
    var podcastID: UUID = UUID()
    var guid: String = ""
    var title: String = ""
    var description: String = ""
    var publishedAt: Date = Date()
    var duration: TimeInterval = 0
    var imageURL: URL? = nil
    var fileURL: URL? = nil
    var isPlayed: Bool = false
    var isDownloaded: Bool = false
    var isStarred: Bool = false
    var playbackPosition: TimeInterval = 0
    var chapters: [Chapter]? = nil
    var transcriptSegments: [TranscriptSegment]? = nil
}

// MARK: - AutoDownloadPolicy

/// Stub policy controlling per-show auto-download behaviour.
struct AutoDownloadPolicy: Equatable, Hashable, Codable {
    enum Mode: Equatable, Hashable, Codable {
        case off
        case latestN(Int)
        case allNew
    }
    var mode: Mode = .off
    var wifiOnly: Bool = true

    static var `default`: AutoDownloadPolicy { AutoDownloadPolicy(mode: .off, wifiOnly: true) }
}

/// T-podcast-gap-001: Full Podcast/Show model backed by kernel snapshot.
struct Podcast: Identifiable, Hashable, Codable {
    let id: UUID
    var title: String = ""
    var author: String = ""
    var description: String = ""
    var imageURL: URL? = nil
    var feedURL: URL? = nil
    /// Episode count from kernel LibraryView snapshot. Zero until kernel
    /// exposes per-podcast episode rows (T-podcast-gap-003).
    var episodeCount: Int = 0
    /// Last RSS refresh timestamp. Nil until kernel exposes refresh metadata.
    var lastRefreshedAt: Date? = nil
    /// Notifications enabled for this show. Stub — always true.
    var notificationsEnabled: Bool = true
    /// Auto-download policy for this show. Stub — returns default policy.
    var autoDownload: AutoDownloadPolicy = .default
    /// Language tag from RSS feed (e.g. "en"). Nil until kernel exposes feed metadata.
    var language: String? = nil

    /// Sentinel used by AllPodcastsListView to exclude the "unknown" fallback row.
    static let unknownID: UUID = UUID(uuidString: "00000000-0000-0000-0000-000000000000")!

    /// Accent colour derived deterministically from title so the grid cells
    /// have stable, distinct tints without querying a palette service.
    var accentColor: Color {
        let colours: [Color] = [.blue, .purple, .pink, .orange, .teal, .indigo, .cyan, .green]
        let hash = abs(title.hashValue)
        return colours[hash % colours.count]
    }

    /// SF Symbol glyph to render when artwork_url is absent.
    var artworkSymbol: String { "antenna.radiowaves.left.and.right" }
}

struct Chapter: Identifiable, Hashable, Codable {
    let id: UUID
    var title: String = ""
    var startTime: TimeInterval = 0
    var imageURL: URL? = nil
    var includeInTableOfContents: Bool = true
}

extension [Chapter] {
    func active(at playhead: TimeInterval) -> Chapter? { nil }
}

struct TranscriptSegment: Identifiable, Hashable, Codable {
    let id: UUID
    var text: String = ""
    var startTime: TimeInterval = 0
}

struct Clip: Identifiable, Hashable, Codable {
    let id: UUID
    var episodeID: UUID = UUID()
    var startSeconds: TimeInterval = 0
    var endSeconds: TimeInterval = 0
    var title: String = ""
}

struct Note: Identifiable, Hashable, Codable {
    let id: UUID
    var text: String = ""
    var createdAt: Date = Date()
    var deleted: Bool = false
}

struct AgentMemory: Identifiable, Hashable, Codable {
    let id: UUID
    var content: String = ""
    var deleted: Bool = false
}

/// Minimal stub for an agent activity log entry.
struct AgentActivityEntry: Identifiable, Hashable, Codable {
    let id: UUID
    var undone: Bool = false
}

struct NostrConversation: Identifiable, Hashable, Codable {
    let id: UUID
    var rootEventID: String = ""
}

/// Stub Nostr friend / contact row.
struct Friend: Identifiable, Hashable, Codable {
    let id: UUID
    var identifier: String = ""  // pubkey hex or npub
    var displayName: String = ""
}

/// Stub pending NIP-46 or agent approval row.
struct NostrPendingApproval: Identifiable, Hashable, Codable {
    let id: UUID
    var pubkeyHex: String = ""
}

// MARK: - AppState

struct AppState: Codable, Sendable {
    /// Initialized with UserDefaults-persisted critical flags so the onboarding
    /// gate survives app restarts. Full Settings disk persistence: T-podcast-gap-004.
    var settings: Settings = {
        var s = Settings()
        s.hasCompletedOnboarding = UserDefaults.standard.bool(forKey: "hasCompletedOnboarding")
        return s
    }()
    var episodes: [Episode] = []
    var podcasts: [Podcast] = []
    var clips: [Clip] = []
    var notes: [Note] = []
    var nostrConversations: [NostrConversation] = []
    var lastPlayedEpisodeID: UUID? = nil

    // MARK: - Fields added for verbatim-3 Settings screen shims

    /// User-followed podcast subscriptions. Stub returns empty list; real
    /// data flows from the kernel library snapshot (T-podcast-gap-003).
    var subscriptions: [Podcast] = []

    /// User-defined podcast categories. Stub returns empty list.
    var categories: [PodcastCategory] = []

    /// Nostr contacts / friends. Stub returns empty list.
    var friends: [Friend] = []

    /// Agent memory entries (long-term memory bank). Stub returns empty.
    var agentMemories: [AgentMemory] = []

    /// Agent activity log entries for the undo subsystem. Stub returns empty.
    var agentActivity: [AgentActivityEntry] = []

    /// Pending NIP-46 / agent approval rows awaiting user decision. Stub empty.
    var nostrPendingApprovals: [NostrPendingApproval] = []
}

// MARK: - AppStateStore

/// T-podcast-ios-3: AppStateStore is @MainActor so it can safely read from
/// KernelModel (which is also @MainActor). Verbatim Podcastr views already
/// run on the main actor; this annotation ensures consistent isolation.
@Observable
@MainActor
final class AppStateStore {
    var state: AppState = AppState()
    var pendingFriendInvite: PendingFriendInvite? = nil

    // T-podcast-ios-3: Kernel model reference injected at startup so
    // allPodcasts reads live from the kernel snapshot instead of the
    // empty state.podcasts stub. Set via bind(kernelModel:) in PodcastApp.
    var _kernelModel: KernelModel?

    /// All known podcasts. Proxied through the kernel snapshot when the
    /// kernel model has been bound; falls back to state.podcasts (empty)
    /// until binding occurs.
    var allPodcasts: [Podcast] {
        guard let km = _kernelModel else { return state.podcasts }
        return km.library.podcasts.compactMap { row in
            guard let uuid = UUID(uuidString: row.id) else { return nil }
            return Podcast(
                id: uuid,
                title: row.title,
                author: row.author,
                imageURL: row.artworkURL,
                feedURL: nil,
                episodeCount: Int(row.episodeCount)
            )
        }
    }

    var allEpisodesSorted: [Episode] { state.episodes.sorted { $0.publishedAt > $1.publishedAt } }

    /// Bind to the kernel model. Called once at app startup so that
    /// allPodcasts and related helpers read live kernel data.
    func bind(kernelModel: KernelModel) {
        _kernelModel = kernelModel
    }

    func podcast(id: UUID) -> Podcast? {
        allPodcasts.first { $0.id == id }
    }

    func podcast(feedURL: URL) -> Podcast? {
        allPodcasts.first { $0.feedURL == feedURL }
    }

    /// All episodes for a given podcast. Returns empty until the kernel
    /// snapshot includes per-podcast episode rows (T-podcast-gap-003).
    func episodes(forPodcast podcastID: UUID) -> [Episode] { [] }

    /// Whether the user actively follows (is subscribed to) a podcast.
    /// Returns a non-nil sentinel when the podcast is in the kernel library,
    /// since every kernel-library row is by definition a subscription.
    func subscription(podcastID: UUID) -> Podcast? {
        allPodcasts.first { $0.id == podcastID }
    }

    /// Remove a podcast subscription from the kernel library.
    /// Routes through the kernel model when bound; no-op otherwise.
    func deletePodcast(podcastID: UUID) {
        guard let km = _kernelModel else { return }
        km.unsubscribe(podcastID: podcastID.uuidString)
    }

    /// Mutates `state.settings` in-place. Called by OnboardingView handlers
    /// and Settings screens to persist preferences. The real Podcastr AppStateStore
    /// persists to disk; this shim updates in-memory state and mirrors
    /// persistence-critical fields to UserDefaults so they survive app restart.
    /// Full Settings disk persistence is T-podcast-gap-004.
    func updateSettings(_ settings: Settings) {
        state.settings = settings
        // Mirror the onboarding-gate flag to UserDefaults so the flow is not
        // shown again after the user completes it. T-podcast-gap-004 covers
        // full disk serialisation of the Settings value.
        UserDefaults.standard.set(settings.hasCompletedOnboarding, forKey: "hasCompletedOnboarding")
    }

    func episode(id: UUID) -> Episode? { nil }
    func clip(id: UUID) -> Clip? { nil }
    func setEpisodePlaybackPosition(_ id: UUID, position: TimeInterval) {}
    func setLastPlayedEpisode(_ id: UUID) {}
    func markEpisodePlayed(_ id: UUID) {}
    func markEpisodeUnplayed(_ id: UUID) {}
    func toggleEpisodeStarred(_ id: UUID) {}
    func setSubscriptionNotificationsEnabled(_ podcastID: UUID, enabled: Bool) {}
    func setSubscriptionAutoDownload(_ podcastID: UUID, policy: AutoDownloadPolicy) {}
    func flushPendingPositions() {}
    func clearTriageDecision(_ id: UUID) {}

    // MARK: - Derived helpers added for verbatim-3 Settings screen shims

    /// Active (non-deleted) notes. Mirrors Podcastr's AppStateStore+DerivedViews.
    var activeNotes: [Note] { state.notes.filter { !$0.deleted } }

    /// Active (non-deleted) agent memories. Mirrors AppStateStore+Memories.
    var activeMemories: [AgentMemory] { state.agentMemories.filter { !$0.deleted } }

    /// Count of non-undone agent activity entries. Mirrors AppStateStore+AgentActivity.
    var activeAgentActivityCount: Int { state.agentActivity.filter { !$0.undone }.count }

    /// Pending Nostr approval rows. Mirrors AppStateStore+Nostr.
    var pendingNostrApprovals: [NostrPendingApproval] { state.nostrPendingApprovals }

    /// Alphabetically-sorted list of subscribed podcasts. Mirrors
    /// AppStateStore+Subscriptions.sortedFollowedPodcasts. Falls back to
    /// allPodcasts (kernel-backed) when state.subscriptions is empty, so the
    /// list is non-empty once the kernel delivers library data.
    var sortedFollowedPodcasts: [Podcast] {
        let base = state.subscriptions.isEmpty ? allPodcasts : state.subscriptions
        return base.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    /// Wipes all user data from AppState. Stub resets in-memory state only.
    /// Real implementation persists deletion (T-podcast-gap-004).
    func clearAllData() {
        state = AppState()
    }

    /// Export OPML data for the subscriptions list. Stub returns nil
    /// (real implementation serialises sortedFollowedPodcasts — T-podcast-gap-004).
    func exportOPML() -> URL? { nil }

    /// Mutates the download state of an episode in place. Stub no-ops since
    /// EpisodeDownloadService owns live state (T-podcast-gap-005).
    func setEpisodeDownloadState(_ id: UUID, state newState: EpisodeDownloadState) {
        // In-memory update for the stub; real impl writes to SQLite persistence.
        guard let idx = state.episodes.firstIndex(where: { $0.id == id }) else { return }
        switch newState {
        case .downloaded: state.episodes[idx].isDownloaded = true
        case .notDownloaded: state.episodes[idx].isDownloaded = false
        default: break
        }
    }
}

struct PendingFriendInvite: Identifiable {
    let id: UUID = UUID()
    var npub: String
    var name: String?
}

// MARK: - UserIdentityStore

@Observable
@MainActor
final class UserIdentityStore {
    static let shared = UserIdentityStore()
    var publicKeyHex: String? = nil

    /// What kind of identity is currently active.
    enum Mode: String, Sendable, Codable {
        case none
        case localKey
        case remoteSigner
    }
    /// Current signer mode. Stub returns .none (no identity configured).
    var mode: Mode = .none

    /// Nostr public key in bech32 npub format. Nil when no identity.
    var npub: String? { nil }

    /// Short display form: first-10 + "…" + last-6 of the full npub.
    var npubShort: String? {
        guard let full = npub, full.count > 16 else { return npub }
        return "\(full.prefix(10))…\(full.suffix(6))"
    }

    func start() {}
}

// MARK: - UserProfileDisplay

struct UserProfileDisplay {
    var displayName: String
    var slug: String   // non-optional in Podcastr API
    var pictureURL: URL?

    static func from(identity: UserIdentityStore) -> UserProfileDisplay? {
        nil
    }
}

// MARK: - NostrRelayService

final class NostrRelayService {
    var agentResponder: AgentResponder = AgentResponder()

    init(store: AppStateStore, askCoordinator: AgentAskCoordinator) {}

    func start() {}
    func republishProfile() {}
}

struct AgentResponder {
    var podcastDepsProvider: (() -> Any)? = nil
    var askCoordinator: AgentAskCoordinator? = nil
}

// MARK: - NostrStack

@MainActor
final class NostrStack {
    static let shared = NostrStack()

    /// Whether any relay WebSocket is currently connected.
    /// Stub returns false (no live relay connections in NMP kernel path).
    private(set) var relaysConnected: Bool = false

    func bind(store: AppStateStore) async {}
    func start() async {}
}

// MARK: - AgentAskCoordinator / AgentChatSession

@Observable
final class AgentAskCoordinator {}

@MainActor
final class AgentChatSession {
    var messages: [String] = []

    init(store: AppStateStore, playback: PlaybackState, askCoordinator: AgentAskCoordinator) {}

    func switchToConversation(_ id: UUID) async {}
}

// MARK: - AgentScheduledTaskRunner

final class AgentScheduledTaskRunner {
    var podcastDepsProvider: (() -> Any)? = nil

    init(store: AppStateStore) {}

    func runDueTasksIfNeeded() {}
}

// MARK: - PlaybackState

@Observable
final class PlaybackState {
    var episode: Episode? = nil
    var autoMarkPlayedOnFinish: Bool = true

    var onPersistPosition: ((UUID, TimeInterval) -> Void)? = nil
    var onEpisodeFinished: ((UUID) -> Void)? = nil
    var onFlushPositions: (() -> Void)? = nil
    var onEnsureDownloadEnqueued: ((UUID) -> Void)? = nil
    var onClearTriageDecision: ((UUID) -> Void)? = nil
    var onSegmentFinished: (() -> Void)? = nil
    var onClipRequested: (() -> Void)? = nil
    var resolveShowName: ((Episode) -> String)? = nil
    var resolveShowImage: ((Episode) -> URL?)? = nil
    var resolveNavigableChapters: ((Episode) -> [Chapter])? = nil

    var engine: PlaybackEngine = PlaybackEngine()

    /// Current playback position in seconds. Zero until real playback exists.
    var currentTime: TimeInterval { 0 }
    /// Queue of episode IDs. Empty stub.
    var queue: [UUID] { [] }

    func setEpisode(_ episode: Episode) {}
    func play() {}
    func pause() {}
    func navigationalSeek(to time: TimeInterval) {}
    func playNext(_ resolver: (UUID) -> Episode?) -> Bool { false }
    func applyPreferences(from settings: Settings) {}
    func isQueued(_ episodeID: UUID) -> Bool { false }
    func enqueue(_ episodeID: UUID) {}
    func removeFromQueue(_ episodeID: UUID) {}
}

final class PlaybackEngine {
    var resolveShowName: ((Episode) -> String?)? = nil
    var resolveActiveChapterTitle: ((Episode, TimeInterval) -> String?)? = nil
    var resolveArtworkURL: ((Episode, TimeInterval) -> URL?)? = nil

    var sleepTimer: SleepTimer = SleepTimer()
}

struct SleepTimer {
    enum Phase { case idle, armedEndOfEpisode, fired }
    var phase: Phase = .idle
}

// MARK: - SpotlightIndexer / DeepLink

enum SpotlightIndexer {
    enum DeepLink: Hashable {
        case note(UUID)
        case memory(UUID)
        case subscription(UUID)
        case episode(UUID)
    }

    static func deepLink(from activity: NSUserActivity) -> DeepLink? { nil }
}

// MARK: - DeepLinkHandler

enum DeepLinkHandler {
    enum Link {
        case settings
        case feedback
        case agent
        case addFriend(npub: String, name: String?)
        case episode(UUID)
        case episodeByGUID(String, startTime: TimeInterval?)
        case subscription(UUID)
        case clip(UUID)
    }

    static func resolve(_ url: URL) -> Link? { nil }
    /// Returns `podcastr://e/<guid>` string. Stub returns nil for empty guid.
    static func episodeGUIDDeepLink(guid: String) -> String? {
        guard !guid.isEmpty else { return nil }
        return "podcastr://e/\(guid)"
    }
    /// Returns a URL with optional timestamp query param. Stub.
    static func episodeGUIDURL(guid: String, startTime: TimeInterval) -> URL? {
        guard !guid.isEmpty else { return nil }
        return URL(string: "podcastr://e/\(guid)?t=\(Int(startTime))")
    }
}

// MARK: - FeedbackWorkflow

/// Must be both @Observable (for RootView's @State var feedbackWorkflow = FeedbackWorkflow())
/// and ObservableObject-compatible (for ScreenshotAnnotationView's @ObservedObject).
/// Using class with @Published so @ObservedObject compiles, and making it compatible
/// with @State by providing an initializer.
final class FeedbackWorkflow: ObservableObject {
    enum Phase { case composing, awaitingScreenshot, annotating }
    @Published var phase: Phase = .composing
    @Published var draft: String = ""
    @Published var screenshot: UIImage? = nil
    @Published var annotatedImage: UIImage? = nil
    var isAnnotationVisible: Bool { phase == .annotating }
}

// MARK: - ShakeFeedbackKit shims

struct ShakeFeedbackStore {
    enum Config { case podcastr }
    init(config: Config, namespace: String) {}
    func start(hostSigner: Any) async {}
}

struct ShakeFeedbackSheet: View {
    let store: ShakeFeedbackStore
    var body: some View { EmptyView() }
}

struct PodcastShakeFeedbackSigner {
    init(identity: UserIdentityStore) {}
}

// MARK: - WhatsNewService / WhatsNewSheet

struct WhatsNewEntry: Identifiable {
    let id: UUID = UUID()
}

final class WhatsNewService {
    static var lastSeenAt: Date { Date() }
    static func seedIfNeeded() {}
    static func unseenEntries(lastSeenAt: Date) -> [WhatsNewEntry] { [] }
}

struct WhatsNewSheet: View {
    let entries: [WhatsNewEntry]
    var body: some View { EmptyView() }
}

// MARK: - CarPlayController

@MainActor
final class CarPlayController {
    static let shared = CarPlayController()
    func attach(store: AppStateStore) {}
}

// MARK: - EpisodeMetadataIndexer

@MainActor
final class EpisodeMetadataIndexer {
    static let shared = EpisodeMetadataIndexer()
    func runBackfill(appStore: AppStateStore) async {}
}

// MARK: - EpisodeDownloadService

@MainActor
final class EpisodeDownloadService {
    static let shared = EpisodeDownloadService()
    /// Live download progress map keyed by episodeID (0-1). Empty stub.
    var progress: [UUID: Double] = [:]
    /// Expected byte counts from URLSession response headers. Empty stub.
    var expectedBytes: [UUID: Int64] = [:]
    func attach(appStore: AppStateStore) {}
    func ensureDownloadEnqueued(episodeID: UUID) {}
    func download(episodeID: UUID) {}
    func cancel(episodeID: UUID) {}
    func delete(episodeID: UUID) {}
    func handleEventsForBackgroundURLSession(identifier: String, completionHandler: @escaping () -> Void) {}
}

// MARK: - AutoSnipController

@MainActor
final class AutoSnipController {
    enum Source { case headphone }
    static let shared = AutoSnipController()
    func captureSnip(source: Source) {}
    func attach(playback: PlaybackState, store: AppStateStore) {}
}

// MARK: - InboxTriageService / ThreadingInferenceService

@MainActor
final class InboxTriageService {
    static let shared = InboxTriageService()
}

@MainActor
final class ThreadingInferenceService {
    static let shared = ThreadingInferenceService()
    struct ActiveTopic: Identifiable {
        let id: UUID = UUID()
    }
}

// MARK: - SubscriptionService

/// Stub subscription service. Real implementation calls Rust podcast-feeds
/// for RSS fetch + parse (T-podcast-gap-003). Today this is an honest no-op
/// that satisfies the type-checker for verbatim Podcastr views.
struct SubscriptionService {
    enum AddError: Error, LocalizedError {
        case alreadySubscribed
        case transport(String)

        var errorDescription: String? {
            switch self {
            case .alreadySubscribed: return "Already subscribed to this podcast."
            case .transport(let msg): return msg
            }
        }
    }

    let store: AppStateStore

    func addSubscription(feedURLString: String) async throws -> Podcast {
        throw AddError.transport("RSS fetch not yet implemented (T-podcast-gap-003)")
    }

    func refresh(_ podcast: Podcast) async {}
}

// MARK: - PodcastCategory

/// Category model mirroring Podcastr's PodcastCategory domain type.
struct PodcastCategory: Identifiable, Hashable, Codable {
    var id: UUID
    var name: String = ""
    var slug: String = ""
    var description: String = ""
    var colorHex: String? = nil
    /// UUIDs of subscriptions placed in this category by the categorisation service.
    var subscriptionIDs: [UUID] = []
    var generatedAt: Date = Date()
}

// MARK: - AppShadow / appShadow view modifier

extension View {
    func appShadow(_ shadow: Any) -> some View { self }
}

// MARK: - AllEpisodesEpisodeList stub

/// Stub list of episodes for the AllEpisodesView. Real implementation renders
/// EpisodeRow × N with kernel-backed episode data (T-podcast-gap-002).
struct AllEpisodesEpisodeList: View {
    let episodes: [Episode]
    let podcastsByID: [UUID: Podcast]
    @Binding var voiceOverDetailRoute: LibraryEpisodeRoute?
    @Binding var visibleCount: Int
    let totalCount: Int

    var body: some View { EmptyView() }
}

// MARK: - Episode property stubs required by Podcastr verbatim views

extension Episode {
    /// Whether this episode has been fully played.
    var played: Bool { isPlayed }
    /// Whether playback has started but not completed.
    var isInProgress: Bool { playbackPosition > 0 && !isPlayed }
    /// Playback progress 0-1. Returns 0 until kernel exposes position data.
    var playbackProgress: Double { 0 }
    /// Publication date. Returns publishedAt (already exists on Episode).
    var pubDate: Date { publishedAt }
    /// Plain-text summary for the episode row. Returns empty until kernel exposes description data.
    var plainTextSummary: String { "" }
    /// Human-readable duration string. Stub returns empty.
    var formattedDuration: String {
        guard duration > 0 else { return "" }
        let h = Int(duration) / 3600
        let m = (Int(duration) % 3600) / 60
        let s = Int(duration) % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
    }
    /// Enclosure URL for sharing. Falls back to a placeholder when fileURL is nil.
    var enclosureURL: URL { fileURL ?? URL(string: "https://example.com/episode")! }

    /// Download state enum matching Podcastr's DownloadState (DownloadState.swift).
    typealias DownloadState = EpisodeDownloadState
    var downloadState: DownloadState { isDownloaded ? .downloaded(localFileURL: fileURL ?? URL(string: "file:///")!, byteCount: 0) : .notDownloaded }

    /// Transcript state enum matching Podcastr semantics.
    enum TranscriptState {
        case none
        case queued
        case fetchingPublisher
        case transcribing(Double)
        case ready
        case failed
    }
    var transcriptState: TranscriptState { .none }
}

// MARK: - LibraryFilter

enum LibraryFilter: String {
    case all
}

// MARK: - HomeEpisodeRoute

struct HomeEpisodeRoute: Hashable {}

// MARK: - EpisodeNavTarget

struct EpisodeNavTarget: Identifiable {
    let id: UUID
}

// MARK: - WikiPage / WikiHomeViewModel

struct WikiPage: Identifiable, Hashable {
    let id: UUID = UUID()
}

@Observable
final class WikiHomeViewModel {
    var searchQuery: String = ""
}

// MARK: - NotificationService

enum NotificationService {
    static let episodeIDUserInfoKey = "episodeID"

    /// Requests user permission for alerts, sound, and badge notifications.
    /// Stub delegates to UNUserNotificationCenter directly. Returns true if granted.
    @MainActor
    static func requestAuthorization() async -> Bool {
        let center = UNUserNotificationCenter.current()
        return (try? await center.requestAuthorization(options: [.alert, .sound, .badge])) ?? false
    }
}

// MARK: - LivePodcastAgentToolDeps

enum LivePodcastAgentToolDeps {
    static func make(store: AppStateStore, playback: PlaybackState) -> Any { () }
}

// MARK: - Notification names used by RootView

extension Notification.Name {
    static let voiceModeRequested = Notification.Name("voiceModeRequested")
    static let askAgentRequested = Notification.Name("askAgentRequested")
    static let openPlayerRequested = Notification.Name("openPlayerRequested")
    static let openSubscriptionDetailRequested = Notification.Name("openSubscriptionDetailRequested")
    static let openAgentChatConversation = Notification.Name("openAgentChatConversation")
    static let openNostrConversationRequested = Notification.Name("openNostrConversationRequested")
}

// MARK: - View modifiers referenced by RootView

extension View {
    // Note: onShake is defined in Design/ShakeDetector.swift (verbatim from Podcastr)
    func nostrApprovalPresenter() -> some View { self }
    func nostrAgentSurface() -> some View { self }
    func agentAskPresenter(coordinator: AgentAskCoordinator) -> some View { self }
    // Note: tabBarMinimizeBehavior is native on iOS 26 (SwiftUI framework)
    // Note: tabViewBottomAccessory is native on iOS 26 (SwiftUI framework)
}

// MARK: - ShakeDetector compatibility
// Note: ShakeDetector.swift handles UIWindow motionEnded override.
// No extension on UIApplication needed here.

// MARK: - PlayerShareSheet stub

/// Stub matching the static helper that EpisodeRowContextMenu references.
/// Real implementation lives in the Player feature (out of scope this iteration).
enum PlayerShareSheet {
    /// Gate threshold: a playhead under 2 seconds is treated as non-meaningful.
    static func isMeaningfulPlayhead(_ time: TimeInterval) -> Bool {
        time > 2.0
    }
}

// MARK: - EpisodeShowNotesFormatter stub

/// Stub matching Podcastr's formatter used by ShowDetailHeader.
/// Returns the raw string stripped of any HTML tags — a very lightweight
/// "plain text" conversion sufficient for the three-line cap in the header.
enum EpisodeShowNotesFormatter {
    static func plainText(from raw: String) -> String {
        // Strip HTML tags with a simple regex-free pass for the stub.
        var result = raw
        while let open = result.firstIndex(of: "<"),
              let close = result[open...].firstIndex(of: ">") {
            result.removeSubrange(open...close)
        }
        return result
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// Note: Double.clamped01 is defined in Design/NumberExtensions.swift

// MARK: - DataExport (verbatim-3)
//
// Pure service used by DataExportView. Verbatim copy from Podcastr would pull
// in the entire AppState Codable chain; instead we ship a functionally
// identical shim that satisfies the type checker and produces valid JSON for
// the honest empty AppState (T-podcast-gap-004).

enum DataExport {

    struct Payload: Codable, Sendable {
        var schemaVersion: Int
        var generatedAt: Date
        var appVersion: String?
        var buildNumber: String?
        var sourceBundleIdentifier: String?
        var state: AppState
    }

    static let currentSchemaVersion = 1

    private static let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.dateEncodingStrategy = .iso8601
        e.outputFormatting = [.prettyPrinted, .sortedKeys]
        return e
    }()

    private static let filenameDateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd-HHmm"
        f.timeZone = TimeZone(identifier: "UTC")
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()

    struct Stats: Sendable, Hashable {
        var subscriptions: Int
        var episodes: Int
        var notes: Int
        var friends: Int
        var memories: Int
        var agentActivity: Int

        var totalRecords: Int {
            subscriptions + episodes + notes + friends + memories + agentActivity
        }
    }

    static func stats(for state: AppState) -> Stats {
        Stats(
            subscriptions: state.subscriptions.count,
            episodes: state.episodes.count,
            notes: state.notes.filter { !$0.deleted }.count,
            friends: state.friends.count,
            memories: state.agentMemories.filter { !$0.deleted }.count,
            agentActivity: state.agentActivity.count
        )
    }

    static func redactedState(from state: AppState) -> AppState {
        var copy = state
        copy.settings.legacyOpenRouterAPIKey = nil
        return copy
    }

    static func makePayload(from state: AppState, now: Date = Date()) -> Payload {
        let info = Bundle.main.infoDictionary
        return Payload(
            schemaVersion: currentSchemaVersion,
            generatedAt: now,
            appVersion: info?["CFBundleShortVersionString"] as? String,
            buildNumber: info?["CFBundleVersion"] as? String,
            sourceBundleIdentifier: Bundle.main.bundleIdentifier,
            state: redactedState(from: state)
        )
    }

    static func encode(_ payload: Payload) throws -> Data {
        try encoder.encode(payload)
    }

    static func suggestedFilename(at date: Date = Date()) -> String {
        "Podcastr-Export-\(filenameDateFormatter.string(from: date)).json"
    }

    static func writeTemporaryFile(_ data: Data, filename: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(filename)
        try data.write(to: url, options: [.atomic])
        return url
    }

    static func writeExport(of state: AppState, now: Date = Date()) throws -> URL {
        let payload = makePayload(from: state, now: now)
        let data = try encode(payload)
        let filename = suggestedFilename(at: now)
        return try writeTemporaryFile(data, filename: filename)
    }
}

// MARK: - OPMLExport (verbatim-3)

/// Minimal OPML exporter for SubscriptionsListView. Mirrors Podcastr's
/// OPMLExport struct — functionally equivalent shim.
struct OPMLExport: Sendable {
    func exportOPML(
        podcasts: [Podcast],
        title: String = "Podcastr Subscriptions",
        dateCreated: Date = Date()
    ) -> Data {
        var lines: [String] = []
        lines.append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")
        lines.append("<opml version=\"2.0\">")
        lines.append("  <head>")
        lines.append("    <title>\(escape(title))</title>")
        lines.append("  </head>")
        lines.append("  <body>")
        lines.append("    <outline text=\"feeds\" title=\"feeds\">")
        for podcast in podcasts {
            if let feedURL = podcast.feedURL {
                let text = escape(podcast.title)
                lines.append("      <outline type=\"rss\" text=\"\(text)\" title=\"\(text)\" xmlUrl=\"\(feedURL.absoluteString)\" />")
            }
        }
        lines.append("    </outline>")
        lines.append("  </body>")
        lines.append("</opml>")
        return lines.joined(separator: "\n").data(using: .utf8) ?? Data()
    }

    private func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;")
         .replacingOccurrences(of: "<", with: "&lt;")
         .replacingOccurrences(of: ">", with: "&gt;")
         .replacingOccurrences(of: "\"", with: "&quot;")
    }
}

// MARK: - PodcastCategorizationService stub (verbatim-3)

/// Categorisation pipeline errors mirroring Podcastr's CategorizationError.
enum CategorizationError: LocalizedError {
    case noAPIKey(provider: String)
    case noSubscriptions
    case noModelSelected
    case invalidResponse
    case httpError(status: Int, body: String)

    var errorDescription: String? {
        switch self {
        case .noAPIKey(let provider):
            return "\(provider) is not connected. Add a key in Settings → Intelligence → Providers."
        case .noSubscriptions:
            return "Add at least one podcast subscription before generating categories."
        case .noModelSelected:
            return "Choose a categorization model in Settings → Intelligence → Models."
        case .invalidResponse:
            return "The model returned an unexpected response. Try again."
        case .httpError(let status, _):
            return "HTTP error \(status). Check your API key and try again."
        }
    }
}

/// Stub category recomputation service used by CategoriesRecomputeSheet.
/// Real implementation calls the LLM categorisation pipeline (T-podcast-gap-006).
@Observable
@MainActor
final class PodcastCategorizationService {
    static let shared = PodcastCategorizationService()

    private(set) var lastRun: Date? = nil
    private(set) var isRunning: Bool = false

    func recompute(store: AppStateStore) async throws -> [PodcastCategory] { [] }
}

// MARK: - AutoDownloadPolicy additions (verbatim-3)

extension AutoDownloadPolicy {
    /// Short human-readable label for the SubscriptionsListView row.
    var summaryLabel: String? {
        switch mode {
        case .off: return nil
        case .latestN(let n): return "Latest \(n)"
        case .allNew: return "All new"
        }
    }
}

// MARK: - EpisodeDownloadStore stub (verbatim-3)

/// Stub matching Podcastr's on-disk download enumerator used by
/// StorageSettingsView.compute(store:). Returns an empty list until a real
/// download engine is wired (T-podcast-gap-005).
@MainActor
final class EpisodeDownloadStore {
    static let shared = EpisodeDownloadStore()

    struct FileEntry: Sendable {
        let url: URL
        let bytes: Int64
        let episodeID: UUID?
    }

    func enumerateOnDisk() -> [FileEntry] { [] }
}

// MARK: - Bridge stub destinations for Settings NavigationLinks (verbatim-3)
//
// These are NavigationLink destinations referenced by the verbatim Settings
// screen that belong to their own verbatim-N iterations. Honest empty-state
// stubs here keep the verbatim SettingsView body byte-for-byte while the
// NavigationLink pushes a recognisable placeholder, not a crash.

/// T-podcast-ios-verbatim-4: Identity root screen. Own verbatim iteration.
struct IdentityRootView: View {
    var body: some View {
        ContentUnavailableView(
            "Identity",
            systemImage: "person.crop.circle",
            description: Text("Identity screen restores in T-podcast-ios-verbatim-4.")
        )
        .navigationTitle("Identity")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// T-podcast-ios-verbatim-N: Categories list. Own verbatim iteration.
struct CategoriesListView: View {
    var body: some View {
        ContentUnavailableView(
            "Categories",
            systemImage: "square.grid.2x2.fill",
            description: Text("Categories screen restores in a future verbatim iteration.")
        )
        .navigationTitle("Categories")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// T-podcast-ios-verbatim-N: Agent settings. Own verbatim iteration.
struct AgentSettingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Agent",
            systemImage: "brain.head.profile",
            description: Text("Agent settings restore in a future verbatim iteration.")
        )
        .navigationTitle("Agent")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// T-podcast-ios-verbatim-N: AI providers settings. Own verbatim iteration.
struct AIProvidersSettingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Providers",
            systemImage: "key.viewfinder",
            description: Text("AI Providers screen restores in a future verbatim iteration.")
        )
        .navigationTitle("Providers")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// T-podcast-ios-verbatim-N: AI models settings. Own verbatim iteration.
struct AIModelsSettingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Models",
            systemImage: "slider.horizontal.3",
            description: Text("AI Models screen restores in a future verbatim iteration.")
        )
        .navigationTitle("Models")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// T-podcast-ios-verbatim-N: Networking settings. Own verbatim iteration.
struct NetworkingSettingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Networking",
            systemImage: "network",
            description: Text("Networking settings restore in a future verbatim iteration.")
        )
        .navigationTitle("Networking")
        .navigationBarTitleDisplayMode(.inline)
    }
}
