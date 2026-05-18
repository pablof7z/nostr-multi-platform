import SwiftUI
import Combine
import CoreSpotlight
import os.log

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

// MARK: - Domain models

/// T-podcast-gap-001: Full Episode model backed by kernel snapshot.
struct Episode: Identifiable, Hashable {
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
struct AutoDownloadPolicy: Equatable, Hashable {
    enum Mode: Equatable, Hashable {
        case off
        case latestN(Int)
        case allNew
    }
    var mode: Mode = .off
    var wifiOnly: Bool = true

    static var `default`: AutoDownloadPolicy { AutoDownloadPolicy(mode: .off, wifiOnly: true) }
}

/// T-podcast-gap-001: Full Podcast/Show model backed by kernel snapshot.
struct Podcast: Identifiable, Hashable {
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

struct Chapter: Identifiable, Hashable {
    let id: UUID
    var title: String = ""
    var startTime: TimeInterval = 0
    var imageURL: URL? = nil
    var includeInTableOfContents: Bool = true
}

extension [Chapter] {
    func active(at playhead: TimeInterval) -> Chapter? { nil }
}

struct TranscriptSegment: Identifiable, Hashable {
    let id: UUID
    var text: String = ""
    var startTime: TimeInterval = 0
}

struct Clip: Identifiable, Hashable {
    let id: UUID
    var episodeID: UUID = UUID()
    var startSeconds: TimeInterval = 0
    var endSeconds: TimeInterval = 0
    var title: String = ""
}

struct Note: Identifiable, Hashable {
    let id: UUID
    var text: String = ""
    var createdAt: Date = Date()
}

struct AgentMemory: Identifiable, Hashable {
    let id: UUID
    var content: String = ""
}

struct NostrConversation: Identifiable, Hashable {
    let id: UUID
    var rootEventID: String = ""
}

// MARK: - AppState

struct AppState {
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

    func start() {}
}

// MARK: - UserProfileDisplay

struct UserProfileDisplay {
    var displayName: String
    var slug: String?
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
    /// Live progress map keyed by episodeID (0-1). Empty stub.
    var progress: [UUID: Double] = [:]
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

/// Stub category model used by LibraryGridCell and HomeCategoryCard.
struct PodcastCategory: Identifiable, Hashable {
    let id: UUID
    var name: String = ""
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

    /// Download state enum with associated values matching Podcastr semantics.
    enum DownloadState {
        case notDownloaded
        /// Persisted progress (0-1) and a transient speed placeholder.
        case downloading(Double, Double)
        case queued
        case downloaded
        case failed
    }
    var downloadState: DownloadState { isDownloaded ? .downloaded : .notDownloaded }

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
