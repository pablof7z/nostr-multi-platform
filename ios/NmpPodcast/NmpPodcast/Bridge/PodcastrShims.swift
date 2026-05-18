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

/// T-podcast-gap-001: Full Podcast/Show model backed by kernel snapshot.
struct Podcast: Identifiable, Hashable {
    let id: UUID
    var title: String = ""
    var author: String = ""
    var imageURL: URL? = nil
    var feedURL: URL? = nil
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

// MARK: - Settings

struct Settings: Equatable {
    /// Reads from UserDefaults so the OnboardingView dismiss sticks across restarts.
    var hasCompletedOnboarding: Bool = UserDefaults.standard.bool(forKey: "hasCompletedOnboarding")
    var nostrEnabled: Bool = false
    var nostrRelayURL: String = ""
    var nostrPublicKeyHex: String? = nil
    var nostrProfileName: String = ""
    var nostrProfileAbout: String = ""
    var nostrProfilePicture: String = ""
    var autoMarkPlayedAtEnd: Bool = true
    var autoPlayNext: Bool = true
}

// MARK: - AppState

struct AppState {
    var settings: Settings = Settings()
    var episodes: [Episode] = []
    var podcasts: [Podcast] = []
    var clips: [Clip] = []
    var notes: [Note] = []
    var nostrConversations: [NostrConversation] = []
    var lastPlayedEpisodeID: UUID? = nil
}

// MARK: - AppStateStore

@Observable
final class AppStateStore {
    var state: AppState = AppState()
    var pendingFriendInvite: PendingFriendInvite? = nil

    var allPodcasts: [Podcast] { state.podcasts }
    var allEpisodesSorted: [Episode] { state.episodes.sorted { $0.publishedAt > $1.publishedAt } }

    func podcast(id: UUID) -> Podcast? { nil }
    func episode(id: UUID) -> Episode? { nil }
    func clip(id: UUID) -> Clip? { nil }
    func setEpisodePlaybackPosition(_ id: UUID, position: TimeInterval) {}
    func setLastPlayedEpisode(_ id: UUID) {}
    func markEpisodePlayed(_ id: UUID) {}
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

    func setEpisode(_ episode: Episode) {}
    func play() {}
    func pause() {}
    func navigationalSeek(to time: TimeInterval) {}
    func playNext(_ resolver: (UUID) -> Episode?) -> Bool { false }
    func applyPreferences(from settings: Settings) {}
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
    func attach(appStore: AppStateStore) {}
    func ensureDownloadEnqueued(episodeID: UUID) {}
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
