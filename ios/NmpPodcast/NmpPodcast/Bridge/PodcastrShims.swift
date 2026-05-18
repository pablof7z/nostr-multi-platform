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
// Domain structs (Episode, Podcast, Clip, Note, etc.) match Podcastr's actual
// field shapes EXACTLY so verbatim view bodies compile without any edits.
// ─────────────────────────────────────────────────────────────────────────────

// MARK: - AutoDownloadPolicy

struct AutoDownloadPolicy: Codable, Sendable, Hashable {
    enum Mode: Codable, Sendable, Hashable {
        case off
        case latestN(Int)
        case allNew
    }
    var mode: Mode
    var wifiOnly: Bool
    init(mode: Mode = .off, wifiOnly: Bool = true) {
        self.mode = mode
        self.wifiOnly = wifiOnly
    }
    static let `default` = AutoDownloadPolicy(mode: .allNew, wifiOnly: true)
    var summaryLabel: String? {
        switch mode {
        case .off: return nil
        case .latestN(let n):
            let base = "Latest \(n)"
            return wifiOnly ? "\(base) · Wi-Fi only" : base
        case .allNew:
            let base = "All new"
            return wifiOnly ? "\(base) · Wi-Fi only" : base
        }
    }
}

// MARK: - DownloadState

enum DownloadState: Codable, Sendable, Hashable {
    case notDownloaded
    case queued
    case downloading(progress: Double, bytesWritten: Int64?)
    case downloaded(localFileURL: URL, byteCount: Int64)
    case failed(message: String)
}

// MARK: - TranscriptState

enum TranscriptState: Codable, Sendable, Hashable {
    case none
    case queued
    case fetchingPublisher
    case transcribing(progress: Double)
    case ready(source: Source)
    case failed(message: String)

    enum Source: String, Codable, Sendable, Hashable {
        case publisher, scribe, whisper, onDevice, assemblyAI, other
    }
}

// MARK: - TriageDecision

enum TriageDecision: String, Codable, Sendable, Hashable, CaseIterable {
    case inbox
    case archived
}

// MARK: - TranscriptKind

enum TranscriptKind: String, Codable, Sendable, Hashable {
    case srt, vtt, txt, json
}

// MARK: - Episode

struct Episode: Codable, Sendable, Identifiable, Hashable {
    var id: UUID
    var podcastID: UUID
    var guid: String
    var title: String
    var description: String
    var pubDate: Date
    var duration: TimeInterval?
    var enclosureURL: URL
    var enclosureMimeType: String?
    var imageURL: URL?
    var chapters: [Episode.Chapter]?
    var persons: [Episode.Person]?
    var soundBites: [Episode.SoundBite]?
    var publisherTranscriptURL: URL?
    var publisherTranscriptType: TranscriptKind?
    var chaptersURL: URL?
    var playbackPosition: TimeInterval
    var played: Bool
    var isStarred: Bool
    var downloadState: DownloadState
    var transcriptState: TranscriptState
    var adSegments: [Episode.AdSegment]?
    var generationSource: Episode.GenerationSource?
    var triageDecision: TriageDecision?
    var triageRationale: String?
    var triageIsHero: Bool
    var metadataIndexed: Bool

    init(
        id: UUID = UUID(),
        podcastID: UUID,
        guid: String,
        title: String,
        description: String = "",
        pubDate: Date,
        duration: TimeInterval? = nil,
        enclosureURL: URL,
        enclosureMimeType: String? = nil,
        imageURL: URL? = nil,
        chapters: [Episode.Chapter]? = nil,
        persons: [Episode.Person]? = nil,
        soundBites: [Episode.SoundBite]? = nil,
        publisherTranscriptURL: URL? = nil,
        publisherTranscriptType: TranscriptKind? = nil,
        chaptersURL: URL? = nil,
        playbackPosition: TimeInterval = 0,
        played: Bool = false,
        isStarred: Bool = false,
        downloadState: DownloadState = .notDownloaded,
        transcriptState: TranscriptState = .none,
        adSegments: [Episode.AdSegment]? = nil,
        generationSource: Episode.GenerationSource? = nil,
        triageDecision: TriageDecision? = nil,
        triageRationale: String? = nil,
        triageIsHero: Bool = false,
        metadataIndexed: Bool = false
    ) {
        self.id = id
        self.podcastID = podcastID
        self.guid = guid
        self.title = title
        self.description = description
        self.pubDate = pubDate
        self.duration = duration
        self.enclosureURL = enclosureURL
        self.enclosureMimeType = enclosureMimeType
        self.imageURL = imageURL
        self.chapters = chapters
        self.persons = persons
        self.soundBites = soundBites
        self.publisherTranscriptURL = publisherTranscriptURL
        self.publisherTranscriptType = publisherTranscriptType
        self.chaptersURL = chaptersURL
        self.playbackPosition = playbackPosition
        self.played = played
        self.isStarred = isStarred
        self.downloadState = downloadState
        self.transcriptState = transcriptState
        self.adSegments = adSegments
        self.generationSource = generationSource
        self.triageDecision = triageDecision
        self.triageRationale = triageRationale
        self.triageIsHero = triageIsHero
        self.metadataIndexed = metadataIndexed
    }

    struct Chapter: Codable, Sendable, Hashable, Identifiable {
        var id: UUID
        var startTime: TimeInterval
        var endTime: TimeInterval?
        var title: String
        var imageURL: URL?
        var linkURL: URL?
        var includeInTableOfContents: Bool
        var isAIGenerated: Bool
        var summary: String?
        var sourceEpisodeID: String?

        init(
            id: UUID = UUID(),
            startTime: TimeInterval,
            endTime: TimeInterval? = nil,
            title: String,
            imageURL: URL? = nil,
            linkURL: URL? = nil,
            includeInTableOfContents: Bool = true,
            isAIGenerated: Bool = false,
            summary: String? = nil,
            sourceEpisodeID: String? = nil
        ) {
            self.id = id; self.startTime = startTime; self.endTime = endTime
            self.title = title; self.imageURL = imageURL; self.linkURL = linkURL
            self.includeInTableOfContents = includeInTableOfContents
            self.isAIGenerated = isAIGenerated; self.summary = summary
            self.sourceEpisodeID = sourceEpisodeID
        }
    }

    struct Person: Codable, Sendable, Hashable, Identifiable {
        var id: UUID; var name: String; var role: String?; var group: String?
        var imageURL: URL?; var linkURL: URL?
        init(id: UUID = UUID(), name: String, role: String? = nil, group: String? = nil, imageURL: URL? = nil, linkURL: URL? = nil) {
            self.id = id; self.name = name; self.role = role; self.group = group
            self.imageURL = imageURL; self.linkURL = linkURL
        }
    }

    struct SoundBite: Codable, Sendable, Hashable, Identifiable {
        var id: UUID; var startTime: TimeInterval; var duration: TimeInterval; var title: String?
        init(id: UUID = UUID(), startTime: TimeInterval, duration: TimeInterval, title: String? = nil) {
            self.id = id; self.startTime = startTime; self.duration = duration; self.title = title
        }
    }

    struct AdSegment: Codable, Sendable, Hashable, Identifiable {
        var id: UUID; var start: TimeInterval; var end: TimeInterval; var kind: AdKind
        init(id: UUID = UUID(), start: TimeInterval, end: TimeInterval, kind: AdKind) {
            self.id = id; self.start = start; self.end = end; self.kind = kind
        }
        enum AdKind: String, Codable, Sendable, Hashable, CaseIterable {
            case preroll, midroll, postroll
        }
    }

    enum GenerationSource: Sendable, Equatable, Hashable, Codable {
        case inAppChat(conversationID: UUID)
        case nostr(rootEventID: String, peerPubkeyHex: String)
    }
}

extension Episode {
    // Triage convenience (not in LibraryDerivedDisplay)
    var isInInbox: Bool { triageDecision == .inbox }
    var isTriageArchived: Bool { triageDecision == .archived }
    var isUntriaged: Bool { triageDecision == nil }
}
// Note: Episode.isUnplayed/isInProgress/playbackProgress/formattedDuration/plainTextSummary
// are defined in verbatim Features/Library/LibraryDerivedDisplay.swift

extension BidirectionalCollection where Element == Episode.Chapter {
    func active(at playheadSeconds: TimeInterval) -> Episode.Chapter? {
        if let hit = self.last(where: { $0.startTime <= playheadSeconds }) { return hit }
        return self.first
    }
}

// MARK: - Podcast

struct Podcast: Codable, Sendable, Identifiable, Hashable {
    enum Kind: String, Codable, Sendable, Hashable { case rss, synthetic }
    enum NostrVisibility: String, Codable, Sendable, Hashable { case `private`, `public` }

    static let unknownID = UUID(uuidString: "00000000-EEEE-EEEE-EEEE-000000000000")!
    static let unknown = Podcast(id: Podcast.unknownID, kind: .synthetic, feedURL: nil, title: "Unknown")

    var id: UUID
    var kind: Kind
    var feedURL: URL?
    var title: String
    var author: String
    var imageURL: URL?
    var description: String
    var language: String?
    var categories: [String]
    var discoveredAt: Date
    var ownerPubkeyHex: String?
    var nostrVisibility: NostrVisibility
    var nostrCoordinate: String?
    var titleIsPlaceholder: Bool
    var lastRefreshedAt: Date?
    var etag: String?
    var lastModified: String?

    init(
        id: UUID = UUID(),
        kind: Kind = .rss,
        feedURL: URL? = nil,
        title: String,
        author: String = "",
        imageURL: URL? = nil,
        description: String = "",
        language: String? = nil,
        categories: [String] = [],
        discoveredAt: Date = Date(),
        lastRefreshedAt: Date? = nil,
        etag: String? = nil,
        lastModified: String? = nil,
        titleIsPlaceholder: Bool = false,
        ownerPubkeyHex: String? = nil,
        nostrVisibility: NostrVisibility = .public,
        nostrCoordinate: String? = nil
    ) {
        self.id = id; self.kind = kind; self.feedURL = feedURL; self.title = title
        self.author = author; self.imageURL = imageURL; self.description = description
        self.language = language; self.categories = categories; self.discoveredAt = discoveredAt
        self.lastRefreshedAt = lastRefreshedAt; self.etag = etag; self.lastModified = lastModified
        self.titleIsPlaceholder = titleIsPlaceholder; self.ownerPubkeyHex = ownerPubkeyHex
        self.nostrVisibility = nostrVisibility; self.nostrCoordinate = nostrCoordinate
    }
}

// Note: Podcast.accentColor/accentHue/artworkSymbol defined in verbatim
// Features/Library/LibraryDerivedDisplay.swift

// MARK: - PodcastSubscription

struct PodcastSubscription: Codable, Sendable, Identifiable, Hashable {
    var podcastID: UUID
    var subscribedAt: Date
    var autoDownload: AutoDownloadPolicy
    var notificationsEnabled: Bool
    var defaultPlaybackRate: Double?
    var id: UUID { podcastID }

    init(podcastID: UUID, subscribedAt: Date = Date(), autoDownload: AutoDownloadPolicy = .default,
         notificationsEnabled: Bool = true, defaultPlaybackRate: Double? = nil) {
        self.podcastID = podcastID; self.subscribedAt = subscribedAt
        self.autoDownload = autoDownload; self.notificationsEnabled = notificationsEnabled
        self.defaultPlaybackRate = defaultPlaybackRate
    }
}

// MARK: - PodcastCategory

struct PodcastCategory: Codable, Sendable, Hashable, Identifiable {
    var id: UUID
    var name: String
    var slug: String
    var description: String
    var colorHex: String?
    var subscriptionIDs: [UUID]
    var generatedAt: Date
    var model: String?

    init(id: UUID = UUID(), name: String, slug: String, description: String = "",
         colorHex: String? = nil, subscriptionIDs: [UUID] = [],
         generatedAt: Date = Date(), model: String? = nil) {
        self.id = id; self.name = name; self.slug = slug; self.description = description
        self.colorHex = colorHex; self.subscriptionIDs = subscriptionIDs
        self.generatedAt = generatedAt; self.model = model
    }
}

// MARK: - Clip

struct Clip: Codable, Sendable, Hashable, Identifiable {
    let id: UUID
    let episodeID: UUID
    let subscriptionID: UUID
    var startMs: Int
    var endMs: Int
    let createdAt: Date
    var caption: String?
    var speakerID: String?
    var transcriptText: String
    var source: Source

    enum Source: String, Codable, Sendable, Hashable {
        case touch, auto, headphone, carplay, watch, siri, agent
    }

    init(id: UUID = UUID(), episodeID: UUID, subscriptionID: UUID, startMs: Int, endMs: Int,
         createdAt: Date = Date(), caption: String? = nil, speakerID: String? = nil,
         transcriptText: String = "", source: Source = .touch) {
        self.id = id; self.episodeID = episodeID; self.subscriptionID = subscriptionID
        self.startMs = startMs; self.endMs = endMs; self.createdAt = createdAt
        self.caption = caption; self.speakerID = speakerID; self.transcriptText = transcriptText
        self.source = source
    }

    var startSeconds: TimeInterval { TimeInterval(startMs) / 1000.0 }
    var endSeconds: TimeInterval { TimeInterval(endMs) / 1000.0 }
    var duration: TimeInterval { Double(endMs - startMs) / 1000 }
    var durationSeconds: TimeInterval { max(0, endSeconds - startSeconds) }
}

// MARK: - NoteKind / NoteAuthor / Anchor / Note

enum NoteKind: String, Codable, Hashable, Sendable { case free, reflection, systemEvent }
enum NoteAuthor: String, Codable, Sendable, Hashable { case user, agent }

enum Anchor: Codable, Hashable, Sendable {
    case note(id: UUID)
    case friend(id: UUID)
    case episode(id: UUID, positionSeconds: TimeInterval)
}

struct Note: Codable, Identifiable, Hashable, Sendable {
    var id: UUID
    var text: String
    var kind: NoteKind
    var target: Anchor?
    var createdAt: Date
    var deleted: Bool
    var author: NoteAuthor

    init(text: String, kind: NoteKind = .free, target: Anchor? = nil, author: NoteAuthor = .user) {
        self.id = UUID(); self.text = text; self.kind = kind; self.target = target
        self.createdAt = Date(); self.deleted = false; self.author = author
    }
}

// MARK: - AgentMemory / CompiledAgentMemory

struct AgentMemory: Codable, Identifiable, Hashable, Sendable {
    var id: UUID; var content: String; var createdAt: Date; var deleted: Bool
    init(content: String) { self.id = UUID(); self.content = content; self.createdAt = Date(); self.deleted = false }
}

struct CompiledAgentMemory: Codable, Hashable, Sendable {
    var text: String; var compiledAt: Date; var sourceMemoryCount: Int; var sourceMemoryIDs: [UUID]
}

// MARK: - Friend

struct Friend: Codable, Identifiable, Hashable, Sendable {
    var id: UUID; var displayName: String; var identifier: String; var addedAt: Date
    var avatarURL: String?; var about: String?
    init(displayName: String, identifier: String) {
        self.id = UUID(); self.displayName = displayName; self.identifier = identifier
        self.addedAt = Date()
    }
    var shortIdentifier: String {
        let half = 8
        guard identifier.count > half * 2 else { return identifier }
        return "\(identifier.prefix(half))…\(identifier.suffix(half))"
    }
}

// MARK: - ThreadingTopic / ThreadingMention

struct ThreadingTopic: Codable, Hashable, Identifiable, Sendable {
    var id: UUID; var slug: String; var displayName: String; var definition: String?
    var episodeMentionCount: Int; var contradictionCount: Int; var lastMentionedAt: Date?
    init(id: UUID = UUID(), slug: String, displayName: String, definition: String? = nil,
         episodeMentionCount: Int = 0, contradictionCount: Int = 0, lastMentionedAt: Date? = nil) {
        self.id = id; self.slug = slug; self.displayName = displayName; self.definition = definition
        self.episodeMentionCount = max(0, episodeMentionCount)
        self.contradictionCount = max(0, contradictionCount); self.lastMentionedAt = lastMentionedAt
    }
}

struct ThreadingMention: Codable, Hashable, Identifiable, Sendable {
    var id: UUID; var topicID: UUID; var episodeID: UUID; var startMS: Int; var endMS: Int
    var snippet: String; var confidence: Double; var isContradictory: Bool
    init(id: UUID = UUID(), topicID: UUID, episodeID: UUID, startMS: Int, endMS: Int,
         snippet: String, confidence: Double = 0.7, isContradictory: Bool = false) {
        self.id = id; self.topicID = topicID; self.episodeID = episodeID
        self.startMS = max(0, startMS); self.endMS = max(self.startMS, endMS)
        self.snippet = snippet; self.confidence = max(0, min(1, confidence))
        self.isContradictory = isContradictory
    }
    var formattedTimestamp: String {
        let totalSeconds = startMS / 1_000
        let hours = totalSeconds / 3_600; let minutes = (totalSeconds % 3_600) / 60; let seconds = totalSeconds % 60
        if hours > 0 { return String(format: "%d:%02d:%02d", hours, minutes, seconds) }
        return String(format: "%d:%02d", minutes, seconds)
    }
}

// Note: Settings struct defined in verbatim Services/Settings.swift

// MARK: - CategorySettings

struct CategorySettings: Codable, Sendable, Hashable {
    var categoryID: UUID
    var autoDownloadOverride: AutoDownloadPolicy?
    var transcriptionEnabled: Bool = true
    var ragEnabled: Bool = true
    var briefingsEnabled: Bool = true
    var wikiEnabled: Bool = true
    init(categoryID: UUID) { self.categoryID = categoryID }
}

// MARK: - Nostr types (stubs for UI compatibility)

struct NostrConversationRecord: Codable, Identifiable, Hashable, Sendable {
    var rootEventID: String; var counterpartyPubkey: String; var firstSeen: Date; var lastTouched: Date
    var turns: [NostrConversationTurn]
    var id: String { rootEventID }
}
struct NostrConversationTurn: Codable, Hashable, Sendable {
    enum Direction: String, Codable, Hashable, Sendable { case incoming, outgoing }
    var eventID: String; var direction: Direction; var pubkey: String; var createdAt: Date; var content: String
    var rawEventJSON: String?
}
struct NostrProfileMetadata: Codable, Equatable, Hashable, Sendable {
    var pubkey: String; var name: String?; var displayName: String?; var about: String?
    var picture: String?; var nip05: String?; var fetchedFromCreatedAt: Int
    var bestLabel: String? { displayName ?? name }
    var pictureURL: URL? { picture.flatMap { URL(string: $0) } }
}
struct NostrPendingApproval: Codable, Identifiable, Hashable, Sendable {
    var id: UUID; var pubkeyHex: String; var displayName: String?; var about: String?
    var pictureURL: String?; var receivedAt: Date; var content: String?
    var shortPubkey: String { pubkeyHex.count > 16 ? "\(pubkeyHex.prefix(8))…\(pubkeyHex.suffix(8))" : pubkeyHex }
    init(pubkeyHex: String, displayName: String? = nil, about: String? = nil, pictureURL: String? = nil, content: String? = nil) {
        self.id = UUID(); self.pubkeyHex = pubkeyHex; self.displayName = displayName
        self.about = about; self.pictureURL = pictureURL; self.receivedAt = Date(); self.content = content
    }
}

// MARK: - AgentActivity

enum AgentActivityKind: Codable, Hashable, Sendable {
    case noteCreated(noteID: UUID)
    case memoryRecorded(memoryID: UUID)
}

struct AgentActivityEntry: Codable, Identifiable, Hashable, Sendable {
    var id: UUID
    var batchID: UUID
    var timestamp: Date
    var kind: AgentActivityKind
    var summary: String
    var undone: Bool

    init(batchID: UUID, kind: AgentActivityKind, summary: String) {
        self.id = UUID()
        self.batchID = batchID
        self.timestamp = Date()
        self.kind = kind
        self.summary = summary
        self.undone = false
    }
}

// MARK: - AgentScheduledTask

struct AgentScheduledTask: Codable, Identifiable, Hashable, Sendable {
    var id: UUID; var label: String; var cronExpression: String?; var nextRunAt: Date?
}

// MARK: - PendingFriendMessage

struct PendingFriendMessage: Codable, Identifiable, Hashable, Sendable {
    var id: UUID; var friendID: UUID; var conversationID: UUID; var sentAt: Date
}

// MARK: - AppState

struct AppState: Codable, Sendable {
    var podcasts: [Podcast] = []
    var subscriptions: [PodcastSubscription] = []
    var episodes: [Episode] = []
    var notes: [Note] = []
    var friends: [Friend] = []
    var agentMemories: [AgentMemory] = []
    var compiledMemory: CompiledAgentMemory? = nil
    var categories: [PodcastCategory] = []
    var categorySettings: [UUID: CategorySettings] = [:]
    var settings: Settings = Settings()
    var nostrAllowedPubkeys: Set<String> = []
    var nostrBlockedPubkeys: Set<String> = []
    var nostrPendingApprovals: [NostrPendingApproval] = []
    var nostrConversations: [NostrConversationRecord] = []
    var nostrProfileCache: [String: NostrProfileMetadata] = [:]
    var nostrRespondedEventIDs: Set<String> = []
    var nostrSinceCursor: Int? = nil
    var agentActivity: [AgentActivityEntry] = []
    var clips: [Clip] = []
    var threadingTopics: [ThreadingTopic] = []
    var threadingMentions: [ThreadingMention] = []
    var agentScheduledTasks: [AgentScheduledTask] = []
    var pendingFriendMessages: [PendingFriendMessage] = []
    var lastPlayedEpisodeID: UUID? = nil
}

// MARK: - EpisodeTriageCounts

struct EpisodeTriageCounts {
    var inbox: Int = 0
    var archived: Int = 0
    var shows: Int = 0
    var isEmpty: Bool { inbox == 0 && archived == 0 }
    mutating func add(_ other: EpisodeTriageCounts) { inbox += other.inbox; archived += other.archived; shows += other.shows }
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
    var unplayedCountByShow: [UUID: Int] = [:]
    var hasDownloadedByShow: Set<UUID> = []
    var hasTranscribedByShow: Set<UUID> = []
    var activeNostrCounterparty: String? = nil

    var allPodcasts: [Podcast] { state.podcasts }
    var allEpisodesSorted: [Episode] { state.episodes.sorted { $0.pubDate > $1.pubDate } }
    var activeNotes: [Note] { state.notes.filter { !$0.deleted } }
    var sortedFollowedPodcasts: [Podcast] { state.subscriptions.compactMap { podcast(id: $0.podcastID) }.sorted { $0.title < $1.title } }
    var sortedFollowedPodcastsByRecency: [Podcast] {
        let podcastByID = Dictionary(uniqueKeysWithValues: state.podcasts.map { ($0.id, $0) })
        let followed = state.subscriptions.compactMap { podcastByID[$0.podcastID] }.filter { $0.kind == .rss }
        return followed.sorted { lhs, rhs in
            let lDate = state.episodes.filter { $0.podcastID == lhs.id }.map(\.pubDate).max()
            let rDate = state.episodes.filter { $0.podcastID == rhs.id }.map(\.pubDate).max()
            switch (lDate, rDate) {
            case let (l?, r?): return l > r
            case (.some, .none): return true
            case (.none, .some): return false
            case (.none, .none): return lhs.title < rhs.title
            }
        }
    }

    var inProgressEpisodes: [Episode] { state.episodes.filter { $0.isInProgress } }

    func bind(kernelModel: Any) {}
    func podcast(id: UUID) -> Podcast? { state.podcasts.first { $0.id == id } }
    func podcast(feedURL: URL) -> Podcast? { state.podcasts.first { $0.feedURL == feedURL } }
    func episode(id: UUID) -> Episode? { state.episodes.first { $0.id == id } }
    func clip(id: UUID) -> Clip? { state.clips.first { $0.id == id } }
    func allClips() -> [Clip] { state.clips.sorted { $0.createdAt > $1.createdAt } }
    func clips(forEpisode id: UUID) -> [Clip] { state.clips.filter { $0.episodeID == id } }
    func episodes(forPodcast id: UUID) -> [Episode] { state.episodes.filter { $0.podcastID == id } }
    func subscription(podcastID: UUID) -> PodcastSubscription? { state.subscriptions.first { $0.podcastID == podcastID } }
    func category(id: UUID) -> PodcastCategory? { state.categories.first { $0.id == id } }
    func category(forPodcast podcastID: UUID) -> PodcastCategory? { state.categories.first { $0.subscriptionIDs.contains(podcastID) } }

    // Note: unplayedCount/hasDownloadedEpisode/hasTranscribedEpisode defined in
    // verbatim Features/Library/LibraryDerivedDisplay.swift

    func triageCounts(allowedSubscriptionIDs: Set<UUID>?) -> EpisodeTriageCounts {
        var counts = EpisodeTriageCounts()
        let episodes = state.episodes.filter { ep in
            guard let allowed = allowedSubscriptionIDs else { return true }
            return allowed.contains(ep.podcastID)
        }
        for ep in episodes {
            switch ep.triageDecision {
            case .inbox: counts.inbox += 1
            case .archived: counts.archived += 1
            case .none: break
            }
        }
        return counts
    }

    func inboxEpisodeIDs(allowedSubscriptionIDs: Set<UUID>?) -> [UUID] {
        state.episodes.filter { ep in
            ep.triageDecision == .inbox
            && (allowedSubscriptionIDs == nil || allowedSubscriptionIDs!.contains(ep.podcastID))
        }.sorted { $0.pubDate > $1.pubDate }.map(\.id)
    }

    func notes(forEpisode episodeID: UUID) -> [Note] { state.notes.filter { n in
        guard let t = n.target, case .episode(let id, _) = t else { return false }
        return id == episodeID && !n.deleted
    }}

    // Mutations (all no-op stubs — logic is in Rust)
    func setSubscriptionNotificationsEnabled(_ podcastID: UUID, enabled: Bool) {}
    func setSubscriptionAutoDownload(_ podcastID: UUID, policy: AutoDownloadPolicy?) {}
    func setEpisodePlaybackPosition(_ id: UUID, position: TimeInterval) {}
    func setLastPlayedEpisode(_ id: UUID) {}
    func markEpisodePlayed(_ id: UUID) {}
    func markEpisodeUnplayed(_ id: UUID) {}
    func toggleEpisodeStarred(_ id: UUID) {}
    func setEpisodeStarred(_ id: UUID, _ starred: Bool) {}
    func flushPendingPositions() {}
    func clearTriageDecision(_ id: UUID) {}
    func deletePodcast(podcastID: UUID) {}
    func resetEpisodeProgress(_ id: UUID) {}
    func deleteClip(id: UUID) {}
    func deleteNote(_ id: UUID) {}
    func setCategories(_ categories: [PodcastCategory]) {}
    func moveSubscription(_ podcastID: UUID, toCategory categoryID: UUID) -> Bool { false }
    func addSubscription(podcastID: UUID) -> Bool { false }
    func mostRecentEpisode(forPodcast podcastID: UUID) -> Episode? { nil }

    @discardableResult
    func addClip(_ clip: Clip) -> Clip { clip }

    @discardableResult
    func addNote(text: String, kind: NoteKind = .free, target: Anchor? = nil) -> Note {
        Note(text: text, kind: kind, target: target)
    }
    @discardableResult
    func addNote(text: String, kind: NoteKind = .free, target: Anchor? = nil, author: NoteAuthor) -> Note {
        Note(text: text, kind: kind, target: target, author: author)
    }

    // MARK: - Agent Activity helpers
    func agentActivity(forBatch batchID: UUID) -> [AgentActivityEntry] {
        state.agentActivity.filter { $0.batchID == batchID }.sorted { $0.timestamp > $1.timestamp }
    }
    var sortedAgentActivity: [AgentActivityEntry] { state.agentActivity.sorted { $0.timestamp > $1.timestamp } }
    var activeAgentActivityCount: Int { state.agentActivity.filter { !$0.undone }.count }
    func recordAgentActivity(_ entry: AgentActivityEntry) { state.agentActivity.append(entry) }
    func undoAgentActivity(_ entryID: UUID) {
        guard let idx = state.agentActivity.firstIndex(where: { $0.id == entryID }) else { return }
        guard !state.agentActivity[idx].undone else { return }
        state.agentActivity[idx].undone = true
    }
    func undoAgentActivityBatch(_ batchID: UUID) {
        let ids = state.agentActivity.filter { $0.batchID == batchID && !$0.undone }.map(\.id)
        for id in ids { undoAgentActivity(id) }
    }
    func pruneStaleActivityEntries() {}
    func deleteAgentMemory(_ id: UUID) {}
}

struct PendingFriendInvite: Equatable, Identifiable {
    let npub: String
    let name: String?
    var id: String { npub }
}

// MARK: - UserIdentityStore

@Observable
@MainActor
final class UserIdentityStore {
    static let shared = UserIdentityStore()
    var publicKeyHex: String? = nil

    enum Mode: Equatable {
        case none
        case localKey
        case remoteSigner
    }

    enum RemoteSignerState: Equatable {
        case idle
        case connecting
        case reconnecting
        case awaitingAuthorization(URL)
        case connected(String)
        case failed(String)
    }

    var mode: Mode = .none
    var remoteSignerState: RemoteSignerState = .idle
    var isRemoteSigner: Bool { mode == .remoteSigner }

    /// Nostr public key in bech32 npub format. Nil when no identity.
    var npub: String? { nil }

    /// Short display form: first-10 + "…" + last-6 of the full npub.
    var npubShort: String? {
        guard let full = npub, full.count > 16 else { return npub }
        return "\(full.prefix(10))…\(full.suffix(6))"
    }

    /// Last login/import failure copy surfaced to the UI. Stub never errors.
    private(set) var loginError: String? = nil

    /// Active signer instance. Nil in the stub (no key configured) — the
    /// verbatim views only check for nil and pass it through to the uploader.
    private(set) var signer: (any NostrSigner)? = nil

    /// Live NIP-46 connection state surfaced by NostrConnectView /
    /// Nip46ConnectCard. Stub stays `.idle`.
    private(set) var remoteSignerState: RemoteSignerState = .idle

    /// Cached kind-0 profile fields. Nil in the stub so UserProfileDisplay
    /// falls back to the deterministic generated profile.
    var profileDisplayName: String? = nil
    var profileName: String? = nil
    var profileAbout: String? = nil
    var profilePicture: String? = nil

    /// True when the active signer is a remote (NIP-46) signer.
    var isRemoteSigner: Bool { mode == .remoteSigner }

    func start() {}

    /// Clears the active identity (stub no-op; resets cached profile fields).
    func clearIdentity() {
        profileDisplayName = nil
        profileName = nil
        profileAbout = nil
        profilePicture = nil
    }

    /// Imports an nsec local key. Stub rejects — no keychain in the NMP path.
    func importNsec(_ nsec: String) throws {
        throw UserIdentityError.noIdentity
    }

    /// Signs + publishes the kind-0 profile event. Stub throws so the
    /// verbatim EditProfileView surfaces its retry banner.
    @discardableResult
    func publishProfile(
        name: String,
        displayName: String,
        about: String,
        picture: String
    ) async throws -> SignedNostrEvent {
        throw UserIdentityError.noIdentity
    }

    /// Connects a remote (bunker) signer from a `bunker://` URI. Stub no-op.
    func connectRemoteSigner(uri: String) async {}

    /// Tears down the active remote signer connection. Stub no-op.
    func disconnectRemoteSigner() async {}

    /// Begins a NIP-46 `nostrconnect://` pairing. Stub never invokes `onURI`.
    func connectViaNostrConnect(
        relay: URL?,
        onURI: @escaping @Sendable (String) -> Void
    ) async {}
}

// MARK: - UserIdentityError

enum UserIdentityError: LocalizedError {
    case noIdentity

    var errorDescription: String? {
        switch self {
        case .noIdentity: return "No identity is configured on this device."
        }
    }
}

// MARK: - RemoteSignerState

/// Mirrors Podcastr's `RemoteSignerState` (Services/UserIdentityStore.swift)
/// byte-for-byte so the verbatim NostrConnectView / Nip46ConnectCard switch
/// statements compile unchanged.
enum RemoteSignerState: Sendable, Equatable {
    case idle
    case connecting
    case reconnecting
    /// The bunker replied with an `auth_url` challenge — the user must approve in a
    /// browser. The connect call itself is still suspended; `connected(...)` follows
    /// once the bunker delivers the real `ack`.
    case awaitingAuthorization(URL)
    case connected(String)            // associated value: user pubkey hex
    case failed(String)               // error message
}

// MARK: - NostrSigner / SignedNostrEvent (Blossom signing hand-off)

/// Opaque signer handle. The verbatim views never call methods on it — they
/// only check `signer != nil` and forward it to `BlossomUploading.upload`.
protocol NostrSigner: Sendable {}

/// Result of a successful kind-0 publish. The verbatim EditProfileView
/// discards the value (`_ = try await identity.publishProfile(...)`).
struct SignedNostrEvent: Sendable {}

// MARK: - BlossomUploading / BlossomUploader

/// Photo upload protocol used by the verbatim ChangePhotoSheet.
protocol BlossomUploading: Sendable {
    func upload(data: Data, contentType: String, signer: any NostrSigner) async throws -> URL
}

/// Default uploader. Stub throws — Blossom upload binds to the Rust kernel
/// in a later slice; the verbatim view surfaces the failure state.
final class BlossomUploader: BlossomUploading {
    init() {}

    func upload(data: Data, contentType: String, signer: any NostrSigner) async throws -> URL {
        throw UserIdentityError.noIdentity
    }
}

// MARK: - Data(hexString:)

extension Data {
    /// Decodes a hex string into bytes. Returns nil on odd length or any
    /// non-hex character. Provided by NDKSwiftCore in Podcastr; reimplemented
    /// here so the verbatim Identity views (AccountDetailsView fingerprint,
    /// Nip46ConnectCard pubkey check) resolve.
    init?(hexString: String) {
        let chars = Array(hexString)
        guard chars.count % 2 == 0 else { return nil }
        var bytes = [UInt8]()
        bytes.reserveCapacity(chars.count / 2)
        var index = chars.startIndex
        while index < chars.endIndex {
            guard let hi = chars[index].hexDigitValue,
                  let lo = chars[chars.index(after: index)].hexDigitValue
            else { return nil }
            bytes.append(UInt8(hi << 4 | lo))
            index = chars.index(index, offsetBy: 2)
        }
        self.init(bytes)
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

    var relaysConnected: Bool = false
    func bind(store: AppStateStore) async {}
    func start() async {}
}

// MARK: - AgentAskCoordinator / AgentChatSession

@Observable
@MainActor
final class AgentAskCoordinator {
    struct PendingAsk: Identifiable, Equatable {
        let id: UUID
        let question: String
        let context: String?
        let createdAt: Date
    }
    private(set) var current: PendingAsk? = nil
    static let timeoutSeconds: TimeInterval = 5 * 60
    func ask(question: String, context: String?) async -> String { "user declined to answer" }
    func resolve(_ id: UUID, with answer: String) {}
    func decline(_ id: UUID) {}
}

@Observable
@MainActor
final class AgentChatSession {
    enum Phase: Equatable {
        case idle
        case sending
        case failed(String)
    }

    var messages: [ChatMessage] = []
    var phase: Phase = .idle
    var loadedFromHistory: Bool = false
    var lastFailedMessage: String?
    var isUpgraded: Bool = false
    var enabledSkills: Set<String> = []
    private(set) var currentConversationID: UUID = UUID()

    let history: ChatHistoryStore = .shared

    init(store: AppStateStore, playback: PlaybackState, askCoordinator: AgentAskCoordinator, history: ChatHistoryStore = .shared) {}

    var streamingContent: String? = nil
    var currentToolName: String? = nil

    var canSend: Bool {
        if case .sending = phase { return false }
        return true
    }
    var canRegenerate: Bool { false }
    func cancelSend() {}
    func startSend(_ text: String, source: AgentRunSource = .typedChat) {}
    func retry() {}
    func regenerateLast() {}
    func startNewConversation() async {}
    func checkAndDrainPendingContext() {}
    func consumeSeededDraftWithAutoSend() -> (draft: String, autoSend: Bool)? { nil }
    func switchToConversation(_ id: UUID) async {}
    func setCurrentConversationID(_ id: UUID) { currentConversationID = id }
}

// MARK: - AgentScheduledTaskRunner

final class AgentScheduledTaskRunner {
    var podcastDepsProvider: (() -> Any)? = nil

    init(store: AppStateStore) {}

    func runDueTasksIfNeeded() {}
}

// MARK: - PlaybackState

// verbatim-5 (#164) reconciled with concurrent ec5310cf Agent/identity shim
// rework: this is the UNION surface — every member the verbatim Player
// View layer (PlayerView/MiniPlayerView/Controls/Scrubber/Sheets) calls,
// plus the members ec5310cf added for its MiniPlayer stub
// (volume / seekForward(seconds:) / seekBackward(seconds:) / skipToChapter /
// non-optional onClearTriageDecision). Honest stub — the kernel/audio seam
// is the verbatim-6 PlaybackState+Audio surface (see orchestration-log).
@MainActor
@Observable
final class PlaybackState {
    let engine: PlaybackEngine = PlaybackEngine()

    var episode: Episode? = nil
    var autoMarkPlayedOnFinish: Bool = true

    var onPersistPosition: ((UUID, TimeInterval) -> Void)? = nil
    var onEpisodeFinished: ((UUID) -> Void)? = nil
    var onFlushPositions: (() -> Void)? = nil
    var onEnsureDownloadEnqueued: ((UUID) -> Void)? = nil
    var onClearTriageDecision: (UUID) -> Void = { _ in }
    var onSegmentFinished: (() -> Void)? = nil
    var onClipRequested: (() -> Void)? = nil
    // Non-optional resolver closures with no-op defaults — surface matches
    // Podcastr's `PlaybackState` exactly so verbatim call sites like
    // `state.resolveShowImage(episode)` compile unchanged.
    var resolveShowName: (Episode) -> String = { _ in "" }
    var resolveShowImage: (Episode) -> URL? = { _ in nil }
    var resolveNavigableChapters: (Episode) -> [Episode.Chapter] = { _ in [] }

    var isPlaying: Bool = false
    var currentTime: TimeInterval = 0
    var duration: TimeInterval = 0
    /// `PlaybackRate` (not `Float`) — matches Podcastr's `PlaybackState.rate`
    /// computed surface so the verbatim `PlayerSpeedSheet` (`rate == current`)
    /// and `PlayerMoreMenu` compile byte-identically. Verified: no host or
    /// ec5310cf shim code reads `state.rate` as Float.
    var rate: PlaybackRate = .normal
    var volume: Float = 1.0
    var queue: [UUID] = []
    var sleepTimer: PlaybackSleepTimer = .off
    var sleepTimerChipLabel: String { "Sleep" }
    var adSegments: [Episode.AdSegment] = []
    var skippedAdSegmentIDs: Set<UUID> = []

    /// Skip intervals the verbatim `PlayerControlsView` reads to label the
    /// skip buttons. `Int` to match Podcastr's `PlaybackState` surface.
    var skipForwardSeconds: Int = 30
    var skipBackwardSeconds: Int = 15

    func setEpisode(_ episode: Episode) { self.episode = episode }
    func play() {}
    func pause() {}
    func togglePlayPause() {}
    func navigationalSeek(to time: TimeInterval) {}
    func seek(to time: TimeInterval) {}
    func seekSnapping(to time: TimeInterval) {}
    func seekForward(seconds: TimeInterval = 30) {}
    func seekBackward(seconds: TimeInterval = 15) {}
    func skipForward() {}
    func skipBackward() {}
    func seekToNextChapter(in navigable: [Episode.Chapter]) {}
    func seekToPreviousChapter(in navigable: [Episode.Chapter]) {}
    func skipToChapter(_ chapter: Episode.Chapter) {}
    func setRate(_ newRate: Double) { rate = PlaybackRate.bestFit(for: newRate) }
    func setRate(_ newRate: PlaybackRate) { rate = newRate }
    func setSleepTimer(_ timer: PlaybackSleepTimer) { sleepTimer = timer }
    func playNext(_ resolver: (UUID) -> Episode?) -> Bool { false }
    func applyPreferences(from settings: Settings) {}
    func isQueued(_ episodeID: UUID) -> Bool { queue.contains(episodeID) }
    func enqueue(_ episodeID: UUID) {}
    func removeFromQueue(_ episodeID: UUID) {}
    func writeNowPlayingSnapshot(force: Bool) {}
}

@MainActor
@Observable
final class PlaybackEngine {
    var resolveShowName: ((Episode) -> String?)? = nil
    var resolveActiveChapterTitle: ((Episode, TimeInterval) -> String?)? = nil
    var resolveArtworkURL: ((Episode, TimeInterval) -> URL?)? = nil
    var currentTime: TimeInterval = 0
    let sleepTimer: SleepTimer = SleepTimer()
}

final class SleepTimer {
    enum Phase: Equatable {
        case idle
        case armed(remaining: TimeInterval)
        case fading(remaining: TimeInterval)
        case armedEndOfEpisode
        case fired
    }
    /// Nested `Mode` mirrors Podcastr's `Audio/SleepTimer.swift` so the
    /// verbatim `PlaybackTypes.swift` `engineMode` mapping compiles unchanged.
    enum Mode: Equatable, Sendable {
        case off
        case duration(TimeInterval)
        case endOfEpisode
    }
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

    /// Builds an `podcastr://friend/add` URL suitable for sharing in an invite
    /// message. Body matches Podcastr's `DeepLinkHandler.friendInviteURL`
    /// verbatim so the verbatim AgentIdentityQRView share path is unchanged.
    static func friendInviteURL(npub: String, name: String?) -> URL? {
        var components = URLComponents()
        components.scheme = "podcastr"
        components.host = "friend"
        components.path = "/add"
        var items: [URLQueryItem] = [URLQueryItem(name: "npub", value: npub)]
        if let name, !name.isEmpty {
            items.append(URLQueryItem(name: "name", value: name))
        }
        components.queryItems = items
        return components.url
    }
}

// Note: FeedbackWorkflow defined in verbatim Features/Feedback/FeedbackWorkflow.swift

// MARK: - ShakeFeedbackKit shims

struct ShakeFeedbackConfig {
    var appName: String
    var clientTag: String
    var projectATag: String
    init(appName: String, clientTag: String, projectATag: String) {
        self.appName = appName; self.clientTag = clientTag; self.projectATag = projectATag
    }
}

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
    func delete(episode: Episode) {}
    func handleEventsForBackgroundURLSession(identifier: String, completionHandler: @escaping () -> Void) {}
}

// MARK: - EpisodeDownloadStore

struct OnDiskFile: Sendable {
    var url: URL
    var episodeID: UUID?
    var bytes: Int64
    init(url: URL, episodeID: UUID? = nil, bytes: Int64 = 0) {
        self.url = url; self.episodeID = episodeID; self.bytes = bytes
    }
}

final class EpisodeDownloadStore: @unchecked Sendable {
    static let shared = EpisodeDownloadStore()
    func allDownloads() -> [UUID] { [] }
    func totalBytes() -> Int64 { 0 }
    func delete(episodeID: UUID) {}
    func enumerateOnDisk() -> [OnDiskFile] { [] }
}

// MARK: - TranscriptIngestService

@MainActor
final class TranscriptIngestService {
    static let shared = TranscriptIngestService()
    func ingest(episodeID: UUID) async {}
    func cancel(episodeID: UUID) {}
    func isIngesting(episodeID: UUID) -> Bool { false }
}

// MARK: - AudioConversationManager

@Observable
@MainActor
final class VoiceCaptionLog {
    private(set) var entries: [VoiceCaption] = []
    func appendPartial(_ speaker: VoiceCaption.Speaker, text: String) -> UUID { UUID() }
    func appendFinal(_ speaker: VoiceCaption.Speaker, text: String) {}
    func update(id: UUID, text: String, stability: VoiceCaption.Stability) {}
    func finalize(id: UUID, text: String) {}
}

@Observable
@MainActor
final class AudioConversationManager {
    static let shared = AudioConversationManager()
    private(set) var state: AudioConversationState = .idle
    private(set) var isAmbient: Bool = false
    private(set) var isUserBargingIn: Bool = false
    let captions = VoiceCaptionLog()
    var isActive: Bool = false
    func start(store: AppStateStore) {}
    func startPushToTalk() {}
    func endPushToTalk() {}
    func stopPushToTalk() {}
    func exitAmbientMode() {}
    func enterAmbientMode() {}
    func interruptCurrentSpeech() {}
    func stop() {}
}

// Note: OPMLExport defined in verbatim Services/OPMLExport.swift

// MARK: - AutoSnipController

@MainActor
final class AutoSnipController {
    enum Source { case headphone, touch }
    static let shared = AutoSnipController()
    func captureSnip(source: Source) {}
    func attach(playback: PlaybackState, store: AppStateStore) {}
}

// verbatim-5 (#164): `AutoSnipButton` is the only `AutoSnip/` symbol the
// kept verbatim `PlayerControlsView` references (`AutoSnipButton()`). The
// rest of `AutoSnip/` is deferred to verbatim-6. Stub preserves the EXACT
// public surface the verbatim call site uses so `PlayerControlsView` stays
// byte-identical; replace byte-for-byte with Podcastr's
// Features/Player/AutoSnip/AutoSnipButton.swift in verbatim-6.
@MainActor
struct AutoSnipButton: View {
    var body: some View {
        Button {
            AutoSnipController.shared.captureSnip(source: .touch)
        } label: {
            Image(systemName: "bookmark.fill")
                .font(.title3.weight(.semibold))
                .foregroundStyle(.primary)
                .frame(width: 44, height: 44)
                .glassEffect(.regular.interactive(), in: .circle)
        }
        .buttonStyle(.pressable)
        .accessibilityLabel("Snip last 30 seconds")
        .accessibilityHint("Saves a 30-second clip ending at the current moment")
    }
}

// MARK: - RAGService (wikiRAG extension)

extension RAGService {
    struct WikiRAGSearch: WikiRAGSearchProtocol {}
    @MainActor var wikiRAG: any WikiRAGSearchProtocol { WikiRAGSearch() }
}

// MARK: - InboxTriageService / ThreadingInferenceService

@MainActor
final class InboxTriageService {
    static let shared = InboxTriageService()
    var isRunning: Bool = false
    var lastCompletedAt: Date? = nil
    func triageNewEpisodes(store: AppStateStore) {}
}

@MainActor
final class ThreadingInferenceService {
    static let shared = ThreadingInferenceService()
    struct ActiveTopic: Sendable, Equatable, Identifiable {
        let topic: ThreadingTopic
        let unplayedEpisodeCount: Int
        var id: UUID { topic.id }
    }
    func attach(store: AppStateStore) {}
    func topActiveTopics(limit: Int, subscriptionFilter: Set<UUID>?) -> [ActiveTopic] { [] }
}

// MARK: - SubscriptionRefreshService

@MainActor
final class SubscriptionRefreshService {
    static let shared = SubscriptionRefreshService()
    func refreshAll(store: AppStateStore) async {}
}

// Note: LibraryFilter defined in verbatim Features/Library/LibraryFilters.swift
// Note: HomeEpisodeRoute defined in verbatim Features/Home/HomeEpisodeRoute.swift
// Note: HomeCategoryScope defined in verbatim Features/Home/HomeCategoryScope.swift
// Note: HomeAgentPick/HomeAgentPicksBundle/HomeInboxBundleBuilder defined in verbatim Features/Home/HomeInboxBundle.swift

// MARK: - EpisodeNavTarget (used in ClippingsView as private, but referenced in PodcastrShims for other uses)
// Note: ClippingsView defines EpisodeNavTarget privately, so we don't need a top-level shim.

// MARK: - WikiPage

struct WikiPage: Codable, Hashable, Identifiable, Sendable {
    static let currentSchemaVersion: Int = 1
    var id: UUID = UUID()
    var slug: String = ""
    var title: String = ""
    var kind: WikiPageKind = .topic
    var scope: WikiScope = .global
    var summary: String = ""
    var sections: [WikiSection] = []
    var citations: [WikiCitation] = []
    var confidence: Double = 0
    var generatedAt: Date = Date()
    var model: String = ""
    var compileRevision: Int = 0
    var schemaVersion: Int = 1
    var isPinned: Bool = false

    var allClaims: [WikiClaim] {
        sections.sorted { $0.ordinal < $1.ordinal }.flatMap(\.claims)
    }
}

// Note: WikiHomeViewModel defined in verbatim Features/Wiki/WikiHomeViewModel.swift

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
    // verbatim-5 (#164): posted by the kept verbatim `PlayerMoreMenu`.
    // Raw value matches Podcastr's deferred
    // `PlayerTranscriptScrollView+AskAgent.swift` byte-for-byte so the
    // verbatim post site compiles unchanged and stays wire-compatible.
    static let openEpisodeDetailRequested = Notification.Name("io.f7z.podcast.openEpisodeDetailRequested")
}

// MARK: - View modifiers referenced by RootView

extension View {
    // Note: onShake is defined in Design/ShakeDetector.swift (verbatim from Podcastr)
    func nostrApprovalPresenter() -> some View { self }
    // Note: nostrAgentSurface() defined in verbatim Features/Agent/NostrAgentSurface.swift
    // Note: agentAskPresenter() defined in verbatim Features/Agent/AgentAskPresenter.swift
}

// Note: EpisodeShowNotesFormatter defined in verbatim Features/EpisodeDetail/EpisodeShowNotesFormatter.swift
// Note: HomeCategoryPickerSheet defined in verbatim Features/Home/HomeCategoryPickerSheet.swift

// MARK: - NDKSwiftCore shims (NDKSwiftCore not linked in NmpPodcast target)

struct NDKRelay: Identifiable, Hashable {
    var id: String { url }
    var url: String = ""
    var managedByNDK: Bool = false
}

enum NDKRelayConnectionState { case connected, disconnected, connecting, failed }
enum NDKRelayOrigin: String { case manual, outbox, inbox }

struct NDKRelaySubscriptionInfo: Identifiable {
    var id: UUID = UUID()
    var filterDescription: String = ""
}

struct NDKRelayInformation {
    var name: String?
    var description: String?
    var pubkey: String?
    var software: String?
    var version: String?
    var contact: String?
    var supportedNIPs: [Int]?
}

struct NDKPrivateKeySigner {
    var privateKeyForNIP59: String = ""
    init(nsec: String) throws {}
    init(privateKey: String) throws {}
    static func generate() throws -> NDKPrivateKeySigner { try NDKPrivateKeySigner(privateKey: "") }
}

// MARK: - Nostr event types (Nip46/NostrSigner.swift types not copied to NmpPodcast)

struct NostrEventDraft: Sendable, Equatable {
    var kind: Int; var content: String; var tags: [[String]]; var createdAt: Int
    init(kind: Int, content: String = "", tags: [[String]] = [], createdAt: Int = Int(Date().timeIntervalSince1970)) {
        self.kind = kind; self.content = content; self.tags = tags; self.createdAt = createdAt
    }
}

struct SignedNostrEvent: Sendable, Equatable, Codable {
    let id: String; let pubkey: String; let created_at: Int; let kind: Int
    let tags: [[String]]; let content: String; let sig: String
    // Note: rootEventID and projectATags defined in verbatim Features/Feedback/FeedbackModels.swift
}

protocol NostrSigner: Sendable {
    func publicKey() async throws -> String
    func sign(_ draft: NostrEventDraft) async throws -> SignedNostrEvent
}

// MARK: - Transcript types (Transcript.swift not copied; used by EpisodeDetail verbatim files)

struct Segment: Codable, Sendable, Hashable, Identifiable {
    let id: UUID
    let start: TimeInterval
    let end: TimeInterval
    let speakerID: UUID?
    let text: String
    let words: [Word]?
    init(id: UUID = UUID(), start: TimeInterval, end: TimeInterval, speakerID: UUID? = nil, text: String, words: [Word]? = nil) {
        self.id = id; self.start = start; self.end = end; self.speakerID = speakerID; self.text = text; self.words = words
    }
}

struct Word: Codable, Sendable, Hashable {
    let start: TimeInterval; let end: TimeInterval; let text: String
    init(start: TimeInterval, end: TimeInterval, text: String) {
        self.start = start; self.end = end; self.text = text
    }
}

struct Speaker: Codable, Sendable, Hashable, Identifiable {
    let id: UUID
    let label: String
    let displayName: String?
    init(id: UUID = UUID(), label: String, displayName: String? = nil) {
        self.id = id; self.label = label; self.displayName = displayName
    }
}

struct Transcript: Codable, Sendable, Hashable {
    var segments: [Segment] = []
    var speakers: [Speaker] = []
    func speaker(for id: UUID?) -> Speaker? {
        guard let id else { return nil }
        return speakers.first { $0.id == id }
    }
}

// MARK: - Domain types referenced by verbatim Home/Wiki/Clippings views

struct Chunk: Sendable, Hashable, Codable, Identifiable {
    var id: UUID = UUID()
    var episodeID: UUID = UUID()
    var podcastID: UUID = UUID()
    var text: String = ""
    var startMS: Int = 0
    var endMS: Int = 0
    var speakerID: UUID? = nil
}

enum ChunkScope: Sendable, Hashable, Codable {
    case all
    case podcast(UUID)
    case episodes(Set<UUID>)
    case episode(UUID)
    case speaker(UUID)
}

struct ChunkMatch: Sendable, Hashable {
    var chunk: Chunk
    var score: Float
    var textHighlights: [Range<String.Index>]
    init(chunk: Chunk = Chunk(), score: Float = 0, textHighlights: [Range<String.Index>] = []) {
        self.chunk = chunk; self.score = score; self.textHighlights = textHighlights
    }
}

enum WikiPageKind: String, Codable, CaseIterable, Sendable {
    case topic, person, show, index
    var displayName: String {
        switch self { case .topic: "Topic"; case .person: "Person"; case .show: "Show"; case .index: "Index" }
    }
}

struct WikiStorage: Sendable {
    let root: URL
    static let inventoryFilename = "_inventory.json"
    init(root: URL? = nil) {
        self.root = root ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!.appendingPathComponent("nmp/wiki")
    }
    static let shared = WikiStorage()
    func write(_ page: WikiPage) throws {}
    func delete(slug: String, scope: WikiScope) throws {}
    func delete(pageID: UUID) throws {}
    func allPages(scope: WikiScope) throws -> [WikiPage] { [] }
    func allPages() throws -> [WikiPage] { [] }
}

struct RAGSearch: Sendable {
    struct Options: Sendable {
        var k: Int; var overfetchMultiplier: Int; var hybrid: Bool; var rerank: Bool
        init(k: Int = 5, overfetchMultiplier: Int = 4, hybrid: Bool = true, rerank: Bool = true) {
            self.k = max(1, k); self.overfetchMultiplier = max(1, overfetchMultiplier)
            self.hybrid = hybrid; self.rerank = rerank
        }
    }
    func search(query: String, scope: ChunkScope? = nil, options: Options = Options()) async throws -> [ChunkMatch] { [] }
}

final class RAGService: @unchecked Sendable {
    static let shared = RAGService()
    let search = RAGSearch()
}

@Observable
@MainActor
final class RationaleNarrator {
    static let shared = RationaleNarrator()
    private(set) var narratingPickID: UUID? = nil
    func attach(playback: PlaybackState) {}
    func narrate(pick: HomeAgentPick, episode: Episode?) async {}
    func speak(pickID: UUID, text: String, voiceID: String, ttsModel: String) async {}
    func stop() {}
}

// MARK: - RelativeTimestamp

enum RelativeTimestamp {
    static func compact(_ date: Date, now: Date = Date()) -> String {
        let interval = max(0, now.timeIntervalSince(date))
        if interval < 5  { return "just now" }
        if interval < 60 { return "\(Int(interval))s ago" }
        if interval < 3_600 { return "\(Int(interval / 60))m ago" }
        return "\(Int(interval / 3_600))h ago"
    }
    static func extended(_ date: Date, now: Date = Date()) -> String {
        let interval = max(0, now.timeIntervalSince(date))
        if interval < 60        { return "just now" }
        if interval < 3_600     { return "\(Int(interval / 60))m ago" }
        if interval < 86_400    { return "\(Int(interval / 3_600))h ago" }
        if interval < 604_800   { return "\(Int(interval / 86_400))d ago" }
        if interval < 2_419_200 { return "\(Int(interval / 604_800))w ago" }
        return date.shortDateTime
    }
}

// MARK: - AppStateStore + Threading helpers

extension AppStateStore {
    func threadingMentions(forTopic id: UUID) -> [ThreadingMention] { [] }
    var pendingNostrApprovals: [NostrPendingApproval] { state.nostrPendingApprovals }
}

// MARK: - Missing View stubs (Settings/Agent leaf destinations not on tab path)

import SwiftUI

struct CategoriesListView: View {
    var body: some View { ContentUnavailableView("Categories", systemImage: "square.grid.2x2") }
}

struct AgentSettingsView: View {
    var body: some View { ContentUnavailableView("Agent Settings", systemImage: "brain") }
}

struct AIModelsSettingsView: View {
    var body: some View { ContentUnavailableView("AI Models", systemImage: "cpu") }
}

struct OpenRouterSettingsView: View {
    var body: some View { ContentUnavailableView("OpenRouter", systemImage: "network") }
}

struct ThreadingTopicListView: View {
    var body: some View { ContentUnavailableView("Topics", systemImage: "link") }
}

struct AIProvidersSettingsView: View {
    var body: some View { ContentUnavailableView("AI Providers", systemImage: "cpu") }
}

struct NostrConversationsView: View {
    var body: some View { ContentUnavailableView("Conversations", systemImage: "bubble.left.and.bubble.right") }
}

struct CategoryDetailView: View {
    @Environment(AppStateStore.self) private var store
    let categoryID: UUID
    var body: some View { ContentUnavailableView("Category", systemImage: "square.grid.2x2") }
}

// MARK: - SubscriptionService shim (logic lives in Rust; Swift side is no-op)

final class SubscriptionService {
    enum AddError: Error, LocalizedError, Equatable {
        case invalidURL
        case alreadySubscribed(title: String)
        case transport(String)
        case http(Int)
        case parse(String)
        var errorDescription: String? {
            switch self {
            case .invalidURL: return "That doesn\'t look like a valid feed URL."
            case .alreadySubscribed(let title): return "You\'re already subscribed to \(title)."
            case .transport(let message): return "Couldn\'t reach the feed: \(message)"
            case .http(let status): return "HTTP \(status)"
            case .parse(let message): return message
            }
        }
    }

    init(store: AppStateStore) {}

    func addSubscription(feedURLString: String) async throws -> Podcast {
        throw AddError.parse("Not available in this build")
    }

    func fetchForAdoption(opmlEntry: Podcast) async throws -> SubscriptionImportPayload? { nil }

    func refresh(podcastID: UUID) async throws {}
    func refresh(_ podcast: Podcast) async {}
}

struct SubscriptionImportPayload {
    var podcast: Podcast
    var episodes: [Episode]
}

// MARK: - AppStateStore + SubscriptionService helpers

extension AppStateStore {
    struct SubscriptionAddResult { var imported: Int; var skipped: Int }
    func addSubscriptions(_ payloads: [SubscriptionImportPayload]) -> SubscriptionAddResult {
        SubscriptionAddResult(imported: 0, skipped: payloads.count)
    }
    func updateSettings(_ settings: Settings) {}
}

// MARK: - Settings mutating helpers

extension Settings {
    mutating func markOpenRouterManual() {}
}

// MARK: - OPMLImport shim

struct OPMLEntry {
    var title: String; var xmlURL: URL?
}

final class OPMLImport {
    func parseOPML(data: Data) throws -> [Podcast] { [] }
}

// MARK: - BYOK shims
// Note: OpenRouterCredentialStore defined in Services/OpenRouterCredentialStore.swift
// Note: PodcastBYOKCredentialImporter defined in Services/BYOKCredentialImporter.swift

// Note: BYOKConnectError defined in Services/BYOKModels.swift
// Note: BYOKConnectService defined in Services/BYOKConnectService.swift

// MARK: - Agent types

struct ChatMessage: Identifiable, Equatable, Codable {
    enum Role: Equatable {
        case user
        case assistant
        case toolBatch(batchID: UUID, count: Int)
        case error
        case skillActivated(skillID: String, displayName: String)
    }
    let id: UUID
    let role: Role
    let text: String
    let timestamp: Date
    init(id: UUID = UUID(), role: Role, text: String, timestamp: Date = Date()) {
        self.id = id; self.role = role; self.text = text; self.timestamp = timestamp
    }
    init(from decoder: Decoder) throws {
        id = UUID(); role = .assistant; text = ""; timestamp = Date()
    }
    func encode(to encoder: Encoder) throws {}
}

// Note: AgentActivityKind and AgentActivityEntry defined above in MARK: - AgentActivity

// Note: LLMProvider, LLMModelReference, LLMProviderCredentialResolver, STTProvider, HeadphoneGestureAction defined in verbatim Services/ files

// MARK: - MarkdownView shim

struct MarkdownView: View {
    let text: String
    var horizontalPadding: CGFloat = 0
    var verticalPadding: CGFloat = 0
    var maxWidth: CGFloat = .infinity
    var alignment: Alignment = .leading
    var controlSize: ControlSize = .regular
    var cornerRadius: CGFloat = 0
    var body: some View { Text(text) }
}

// MARK: - ChatConversation / ChatHistoryStore shims

struct ChatConversation: Identifiable, Codable, Equatable, Sendable {
    let id: UUID
    var title: String = ""
    var messages: [ChatMessage] = []
    var isUpgraded: Bool = false
    var enabledSkills: Set<String> = []
    var isScheduledTask: Bool = false
    let createdAt: Date
    var updatedAt: Date
    init(id: UUID = UUID(), title: String = "", createdAt: Date = Date()) {
        self.id = id; self.title = title; self.createdAt = createdAt; self.updatedAt = createdAt
    }
}

@MainActor
final class ChatHistoryStore {
    static let shared = ChatHistoryStore()
    var conversations: [ChatConversation] = []
    var mostRecent: ChatConversation? { nil }
    func conversation(id: UUID) -> ChatConversation? { nil }
    func save(_ conversation: ChatConversation) {}
    func delete(id: UUID) {}
}

// MARK: - AgentChatHistoryView shim

struct AgentChatHistoryView: View {
    let history: ChatHistoryStore
    let currentID: UUID
    let onSelect: (ChatConversation) -> Void
    let onNew: () -> Void
    var body: some View { ContentUnavailableView("History", systemImage: "clock") }
}

// MARK: - AgentChatTranscriptExport shim

enum AgentChatTranscriptExport {
    static func write(_ messages: [ChatMessage], batchSummaries: [UUID: [String]] = [:]) -> URL? { nil }
    static func format(_ messages: [ChatMessage], batchSummaries: [UUID: [String]] = [:]) -> String { "" }
}

// MARK: - Agent tool types

enum AgentRunSource: String, Codable, Sendable {
    case typedChat, voiceMessage, nostrInbound, manual, scheduledTask
}

enum AgentRunOutcome: String, Codable, Sendable {
    case completed, turnsExhausted, failed, cancelled
}

enum AgentSkillID {
    static let podcastGeneration = "podcast_generation"
    static let wikiResearch = "wiki_research"
    static let conversationHistory = "conversation_history"
    static let youtubeIngestion = "youtube_ingestion"
}

enum AgentTools {
    enum Names {
        static let createNote       = "create_note"
        static let recordMemory     = "record_memory"
        static let upgradeThinking  = "upgrade_thinking"
        static let useSkill         = "use_skill"
        static let ask              = "ask"
        static let scheduleTask     = "schedule_task"
        static let cancelScheduledTask = "cancel_scheduled_task"
        static let listScheduledTasks  = "list_scheduled_tasks"
        static let listConversations   = "list_conversations"
        static let searchConversations = "search_conversations"
    }
    static let summaryTruncationLength = 40
    static func toolSuccess(_ payload: [String: Any] = [:]) -> String { "{\"success\":true}" }
    static func toolError(_ message: String) -> String { "{\"error\":\"\(message)\"}" }
    static func truncated(_ s: String) -> String {
        s.count > summaryTruncationLength ? "\(s.prefix(summaryTruncationLength))…" : s
    }
}

// MARK: - Voice types

enum AudioConversationState: Equatable, Sendable {
    case idle
    case listening
    case thinking
    case speaking
    case duckedWhileBriefing
    case error(VoiceError)
}

// MARK: - VoiceBriefingHandle

@MainActor
protocol VoiceBriefingHandle: AnyObject {
    func waitUntilFinished() async
}

enum VoiceError: Error, Equatable, Sendable {
    case permissionDenied
    case recognizerUnavailable
    case ttsFailed(String)
    case agentFailed(String)
    case audioRouteFailed(String)
    case unknown(String)
}

struct VoiceCaption: Identifiable, Equatable, Sendable {
    enum Speaker: String, Sendable, Equatable { case user, agent }
    enum Stability: String, Sendable, Equatable { case partial, final }
    let id: UUID
    let speaker: Speaker
    let text: String
    let stability: Stability
    init(id: UUID = UUID(), speaker: Speaker, text: String, stability: Stability) {
        self.id = id; self.speaker = speaker; self.text = text; self.stability = stability
    }
}

// MARK: - Blossom / Photo upload

protocol BlossomUploading: Sendable {
    func upload(data: Data, contentType: String, signer: any NostrSigner) async throws -> URL
}

// MARK: - iTunes discovery

enum ITunesSearchClient {
    struct Result: Decodable, Sendable, Hashable, Identifiable {
        let collectionId: Int
        let collectionName: String
        let artistName: String?
        let feedUrl: String?
        let artworkUrl600: String?
        let artworkUrl100: String?
        let primaryGenreName: String?
        let trackCount: Int?
        var id: Int { collectionId }
        var feedURL: URL? { feedUrl.flatMap { URL(string: $0) } }
        var artworkURL: URL? {
            if let s = artworkUrl600, let u = URL(string: s) { return u }
            if let s = artworkUrl100, let u = URL(string: s) { return u }
            return nil
        }
    }
    static func search(term: String) async throws -> [Result] { [] }
}

// MARK: - Wiki content types

struct WikiCitation: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var episodeID: UUID = UUID()
    var startMS: Int = 0
    var endMS: Int = 0
    var quoteSnippet: String = ""
    var speaker: String? = nil
    var verificationConfidence: WikiConfidenceBand = .medium

    var durationMS: Int { max(0, endMS - startMS) }
    var formattedTimestamp: String {
        let totalSeconds = startMS / 1_000
        let hours = totalSeconds / 3_600
        let minutes = (totalSeconds % 3_600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 { return String(format: "%d:%02d:%02d", hours, minutes, seconds) }
        return String(format: "%d:%02d", minutes, seconds)
    }
}

enum WikiConfidenceBand: String, Codable, Hashable, Sendable, CaseIterable {
    case high, medium, low

    var label: String {
        switch self {
        case .high: "high"
        case .medium: "medium"
        case .low: "low"
        }
    }

    var accessibilityValue: String {
        switch self {
        case .high: "high evidence"
        case .medium: "medium evidence"
        case .low: "low evidence"
        }
    }
}

struct WikiClaim: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var text: String = ""
    var citations: [WikiCitation] = []
    var confidence: WikiConfidenceBand = .medium
    var isGeneralKnowledge: Bool = false
    var isContestedByUser: Bool = false
}

enum WikiSectionKind: String, Codable, Hashable, Sendable {
    case definition, consensus, contradictions, citations, other
}

struct WikiSection: Codable, Hashable, Identifiable, Sendable {
    var id: UUID = UUID()
    var heading: String = ""
    var kind: WikiSectionKind = .other
    var ordinal: Int = 0
    var claims: [WikiClaim] = []
    var editorialNote: String? = nil
}

enum WikiScope: Codable, Hashable, Sendable {
    case global
    case subscription(UUID)
    case episode(UUID)
    case podcast(UUID)

    var pathComponent: String {
        switch self {
        case .global: "global"
        case .subscription(let id): "subscription_\(id.uuidString)"
        case .episode(let id): "episode_\(id.uuidString)"
        case .podcast(let id): "podcast_\(id.uuidString)"
        }
    }
}

struct WikiVerifyResult: Sendable {
    var page: WikiPage
    init(page: WikiPage) { self.page = page }
}

protocol WikiRAGSearchProtocol: Sendable {}

struct WikiOpenRouterClient {
    static func live(model: String) -> WikiOpenRouterClient { WikiOpenRouterClient() }
    func generate(topic: String, scope: WikiScope, citations: [WikiCitation]) async throws -> WikiPage {
        WikiPage()
    }
}

struct WikiGenerator: Sendable {
    init(rag: any WikiRAGSearchProtocol, client: WikiOpenRouterClient = .live(model: ""), storage: WikiStorage = .shared, model: String = "") {}
    init(storage: WikiStorage, rag: RAGSearch, llm: LLMModelReference) {}
    func generate(for page: WikiPage, scope: WikiScope) async throws -> WikiPage { page }
    func verify(_ page: WikiPage) async throws -> WikiVerifyResult { WikiVerifyResult(page: page) }
    func audit(prior page: WikiPage) async throws -> WikiVerifyResult { WikiVerifyResult(page: page) }
    func persist(_ page: WikiPage) throws {}
    func compileTopic(topic: String, scope: WikiScope) async throws -> WikiVerifyResult { WikiVerifyResult(page: WikiPage()) }
    func compilePerson(name: String, scope: WikiScope) async throws -> WikiVerifyResult { WikiVerifyResult(page: WikiPage()) }
    func compileShow(showName: String, scope: WikiScope) async throws -> WikiVerifyResult { WikiVerifyResult(page: WikiPage()) }
}

enum WikiGeneratorError: LocalizedError {
    case insufficientEvidence(query: String)
    case unknown
    var errorDescription: String? {
        switch self {
        case .insufficientEvidence(let q): return "Insufficient evidence to generate a wiki page for \(q)."
        case .unknown: return "Unknown wiki generator error."
        }
    }
}

enum WikiClientError: LocalizedError {
    case missingCredential(provider: String)
    case httpError(status: Int, body: String)
    case malformedResponse
    var errorDescription: String? {
        switch self {
        case .missingCredential(let provider): return "\(provider) is not connected. Add a key in Settings."
        case .httpError(let status, let body): return "Wiki API error (\(status)): \(String(body.prefix(200)))"
        case .malformedResponse: return "Malformed response from wiki API"
        }
    }
}

// Note: DataExport defined in verbatim Services/DataExport.swift

// MARK: - Podcast categorization

enum CategorizationError: LocalizedError {
    case noAPIKey(provider: String)
    case noSubscriptions
    case noModelSelected

    var errorDescription: String? {
        switch self {
        case .noAPIKey(let provider): return "No API key for \(provider)."
        case .noSubscriptions: return "No subscriptions to categorize."
        case .noModelSelected: return "No model selected."
        }
    }
}

@Observable
@MainActor
final class PodcastCategorizationService {
    static let shared = PodcastCategorizationService()
    var isRunning: Bool = false
    private(set) var lastRun: Date? = nil
    func recompute(store: AppStateStore) async throws {}
}

// MARK: - Nostr podcast discovery

@Observable
@MainActor
final class NostrPodcastDiscoveryService {
    struct ShowResult: Identifiable, Sendable {
        var id: String { pubkey }
        let coordinate: String
        let pubkey: String
        let title: String
        let author: String
        let imageURL: URL?
        let description: String
        let categories: [String]
        let createdAt: Int
    }
    var isLoading: Bool = false
    var shows: [ShowResult] = []
    func fetchShows(relayURL: URL) async -> [ShowResult] { [] }
    func subscribe(to show: ShowResult, store: AppStateStore, relayURL: URL) async -> Podcast {
        Podcast(feedURL: nil, title: show.title)
    }
    static func podcastID(for coordinate: String) -> UUID { UUID() }
}

// MARK: - Episode audit log

@MainActor
final class EpisodeAuditLogStore {
    static let shared = EpisodeAuditLogStore()
    func entries(for episodeID: UUID) -> [AuditLogEntry] { [] }
    struct AuditLogEntry: Identifiable {
        var id: UUID = UUID()
        var timestamp: Date = Date()
        var summary: String = ""
    }
}

// MARK: - NotificationService (extended)

extension NotificationService {
    static func requestAuthorization() async -> Bool { false }
    static func scheduleEpisodeNotification(episode: Episode, podcast: Podcast) async {}
}

// MARK: - TranscriptStore / ChaptersHydration / AIChapterCompiler shims

final class TranscriptStore: @unchecked Sendable {
    static let shared = TranscriptStore()
    func save(_ transcript: Transcript) throws {}
    func load(episodeID: UUID) -> Transcript? { nil }
    func delete(episodeID: UUID) {}
}

@MainActor
final class ChaptersHydrationService {
    static let shared = ChaptersHydrationService()
    func hydrateIfNeeded(episode: Episode, store: AppStateStore) {}
}

@MainActor
final class AIChapterCompiler {
    static let shared = AIChapterCompiler()
    func compileIfNeeded(episodeID: UUID, store: AppStateStore) async {}
}

// MARK: - PlayerShareSheet shim

struct PlayerShareSheet: View {
    @Bindable var state: PlaybackState
    let episode: Episode
    let showName: String
    var body: some View { EmptyView() }
    static func isMeaningfulPlayhead(_ currentTime: TimeInterval) -> Bool { currentTime > 5 }
}

// Note: WikiEvidenceGrade defined in verbatim Features/Wiki/EvidenceGradedRule.swift
// Note: FeedbackThread / FeedbackReply defined in verbatim Features/Feedback/FeedbackModels.swift

// MARK: - UserIdentityStore feedback/identity helpers

extension UserIdentityStore {
    var hasIdentity: Bool { publicKeyHex != nil }
    var npub: String? {
        guard let hex = publicKeyHex, let bytes = Data(hexString: hex), bytes.count == 32 else { return nil }
        return Bech32.encode(hrp: "npub", data: bytes)
    }
    var npubShort: String? {
        guard let full = npub, full.count > 16 else { return npub }
        return "\(full.prefix(10))…\(full.suffix(6))"
    }
    func publishFeedbackNote(category: FeedbackCategory, body: String, parentEventID: String?, replyToPubkey: String?) async throws -> SignedNostrEvent {
        throw NSError(domain: "stub", code: 0)
    }
}

// MARK: - BlossomUploader shim

struct BlossomUploader: BlossomUploading {
    static let defaultServer = URL(string: "https://blossom.primal.net")!
    func upload(data: Data, contentType: String, signer: any NostrSigner) async throws -> URL {
        throw NSError(domain: "stub", code: 0)
    }
}

// Note: ElevenLabsCredentialStore, AssemblyAICredentialStore, OpenRouterCredentialStore defined in verbatim Services/ files

// MARK: - EpisodeDownloadService extras

extension EpisodeDownloadService {
    static func formatBytes(_ bytes: Int64) -> String {
        let kb = Double(bytes) / 1024
        let mb = kb / 1024
        if mb >= 1 { return String(format: "%.1f MB", mb) }
        if kb >= 1 { return String(format: "%.0f KB", kb) }
        return "\(bytes) B"
    }
}

// MARK: - EpisodeAuditEvent.Kind extra

extension EpisodeAuditEvent.Kind {
    var iconName: String {
        switch rawValue {
        case "transcript.retry": return "arrow.clockwise"
        default: return "info.circle"
        }
    }
    var displayLabel: String {
        switch rawValue {
        case "transcript.retry": return "Retry requested"
        default: return rawValue.replacingOccurrences(of: ".", with: " ").capitalized
        }
    }
}

// MARK: - AppStateStore transcript state

extension AppStateStore {
    func setEpisodeTranscriptState(_ id: UUID, state: TranscriptState) {}
}

// MARK: - TranscriptIngestService expanded

extension TranscriptIngestService {
    func ingest(episodeID: UUID, forceProvider: STTProvider?) async {}
}

// MARK: - NostrProfileFetcher shim

@MainActor
final class NostrProfileFetcher {
    static let shared = NostrProfileFetcher()
    init() {}
    init(store: AppStateStore) {}
    func fetchProfile(pubkeyHex: String) async -> NostrProfileMetadata? { nil }
    func fetchProfiles(for pubkeys: [String]) async {}
}

// MARK: - VoiceNoteRealtimeSTT shim

@MainActor
final class VoiceNoteRealtimeSTT: ObservableObject {
    @Published private(set) var isRecording: Bool = false
    @Published private(set) var isStarting: Bool = false
    @Published private(set) var level: Float = 0
    @Published private(set) var transcript: String = ""
    @Published private(set) var errorMessage: String? = nil
    @Published private(set) var statusMessage: String = "Idle"
    func start(modelID: String = "") async throws {}
    func stop() async -> String { transcript }
    func cancel() {}
}

// MARK: - EpisodeAuditEvent shim

struct EpisodeAuditEvent: Codable, Sendable, Hashable, Identifiable {
    var id: UUID = UUID()
    var episodeID: UUID = UUID()
    var timestamp: Date = Date()
    var kind: Kind = "info"
    var severity: Severity = .info
    var summary: String = ""
    var details: [Detail] = []

    struct Detail: Codable, Sendable, Hashable {
        var label: String; var value: String
        init(_ label: String, _ value: String) { self.label = label; self.value = value }
    }
    enum Severity: String, Codable, Sendable, Hashable {
        case info, success, warning, failure
    }
    struct Kind: Codable, Sendable, Hashable, RawRepresentable, ExpressibleByStringLiteral {
        let rawValue: String
        init(rawValue: String) { self.rawValue = rawValue }
        init(stringLiteral value: String) { self.rawValue = value }
        static let transcriptRetryRequested: Kind = "transcript.retry"
    }

    init(id: UUID = UUID(), episodeID: UUID, timestamp: Date = Date(), kind: Kind, severity: Severity = .info, summary: String, details: [Detail] = []) {
        self.id = id; self.episodeID = episodeID; self.timestamp = timestamp
        self.kind = kind; self.severity = severity; self.summary = summary; self.details = details
    }
}

// MARK: - EpisodeAuditLogStore (expanded)

extension EpisodeAuditLogStore {
    func eventsNewestFirst(for episodeID: UUID) -> [EpisodeAuditEvent] { [] }
    func clear(episodeID: UUID) {}
    @discardableResult
    func record(episodeID: UUID, kind: EpisodeAuditEvent.Kind, severity: EpisodeAuditEvent.Severity = .info, summary: String, details: [EpisodeAuditEvent.Detail] = []) -> EpisodeAuditEvent {
        EpisodeAuditEvent(episodeID: episodeID, kind: kind, severity: severity, summary: summary, details: details)
    }
}

// MARK: - EpisodeDownloadService (expanded)

extension EpisodeDownloadService {
    var expectedBytes: [UUID: Int64] { [:] }
}

// MARK: - AppStateStore download state

extension AppStateStore {
    func setEpisodeDownloadState(_ id: UUID, state: DownloadState) {}
    func clearAllData() {}
    var activeMemories: [String] { [] }
    // Note: activeAgentActivityCount defined in main AppStateStore body
}

// MARK: - UserIdentityStore (expanded)

extension UserIdentityStore {
    var profileDisplayName: String? { nil }
    var signer: (any NostrSigner)? { nil }
    func publishProfile(name: String?, displayName: String?, about: String?, picture: String?) async throws -> SignedNostrEvent? { nil }
}

// MARK: - ITunesSearchClient (expanded)

extension ITunesSearchClient {
    static func search(_ term: String) async throws -> [Result] { [] }
    static func topPodcasts() async throws -> [Result] { [] }
}

// verbatim-5 (#164): queue methods (isQueued/enqueue/removeFromQueue) merged
// into the reconciled `PlaybackState` class above; the standalone extension
// from concurrent ec5310cf was removed here to avoid redeclaration.

// MARK: - EpisodeComment / NostrCommentService

enum CommentTarget: Hashable, Sendable {
    case episode(guid: String)
    case clip(id: UUID)
}

struct EpisodeComment: Identifiable, Hashable, Sendable {
    let id: String
    let target: CommentTarget
    let authorPubkeyHex: String
    let content: String
    let createdAt: Date
    var authorShortKey: String { authorPubkeyHex.prefix(4) + "…" + authorPubkeyHex.suffix(4) }
}

@MainActor
final class NostrCommentService {
    init(relayURLProvider: @escaping () -> URL? = { nil }) {}
    init(store: AppStateStore) {}

    struct Subscription {
        var stream: AsyncStream<EpisodeComment> { AsyncStream { _ in } }
        var comments: AsyncStream<[EpisodeComment]> { AsyncStream { _ in } }
        func cancel() {}
    }

    func subscribe(target: CommentTarget) -> Subscription { Subscription() }
    func publish(content: String, target: CommentTarget, signer: any NostrSigner) async throws -> SignedNostrEvent {
        throw NSError(domain: "stub", code: 0)
    }
}

// MARK: - Bech32 encoding shim

enum Bech32 {
    static func encode(hrp: String, data: Data) -> String {
        "\(hrp)1" + data.map { String(format: "%02x", $0) }.joined()
    }
}

// MARK: - Data hex init shim

extension Data {
    init?(hexString hex: String) {
        let len = hex.count
        guard len % 2 == 0 else { return nil }
        var data = Data(capacity: len / 2)
        var i = hex.startIndex
        while i < hex.endIndex {
            let j = hex.index(i, offsetBy: 2)
            guard let byte = UInt8(hex[i..<j], radix: 16) else { return nil }
            data.append(byte)
            i = j
        }
        self = data
    }
}

// MARK: - Nostr UI shims

struct NostrProfileAvatar: View {
    var profile: NostrProfileMetadata?
    var body: some View {
        Image(systemName: "person.circle.fill")
            .resizable()
            .foregroundStyle(.secondary)
    }
}

enum NostrNpub {
    static func shortNpub(fromHex hex: String) -> String {
        let prefix = hex.prefix(8)
        let suffix = hex.suffix(4)
        return "npub\(prefix)…\(suffix)"
    }
}

// MARK: - DownloadProgressBadge shim

struct DownloadProgressBadge: View {
    var episode: Episode
    var liveProgress: Double?
    var body: some View {
        let progress = liveProgress ?? 0
        ZStack {
            Circle()
                .stroke(Color.secondary.opacity(0.3), lineWidth: 2)
            Circle()
                .trim(from: 0, to: progress)
                .stroke(Color.accentColor, lineWidth: 2)
                .rotationEffect(.degrees(-90))
        }
    }
}

// MARK: - BriefingScope / BriefingLength / BriefingStyle / BriefingRequest

enum BriefingScope: String, Codable, Sendable, Hashable, CaseIterable {
    case mySubscriptions
    case thisShow
    case thisTopic
    case thisWeek

    var displayName: String {
        switch self {
        case .mySubscriptions: "my subscriptions"
        case .thisShow:        "this show"
        case .thisTopic:       "this topic"
        case .thisWeek:        "this week"
        }
    }
}

enum BriefingLength: String, Codable, Sendable, Hashable, CaseIterable {
    case quick
    case medium
    case long
    case extended
}

enum BriefingStyle: String, Codable, Sendable, Hashable, CaseIterable {
    case morning
    case weeklyTLDR
    case catchUpOnShow
    case topicAcrossLibrary
}

struct BriefingRequest: Codable, Sendable, Hashable, Identifiable {
    var id: UUID = UUID()
    var scope: BriefingScope = .mySubscriptions
    var length: BriefingLength = .medium
    var style: BriefingStyle = .morning
    var freeformQuery: String? = nil
    var requestedAt: Date = Date()
}

// MARK: - BriefingComposeSheet

struct BriefingComposeSheet: View {
    var onCompose: (BriefingRequest) -> Void = { _ in }
    var initialFreeformQuery: String = ""
    var initialScope: BriefingScope? = nil
    var body: some View {
        ContentUnavailableView("Briefing", systemImage: "doc.text")
    }
}

// MARK: - AgentIdentityQRView

struct AgentIdentityQRView: View {
    let npub: String
    let name: String
    var body: some View {
        ContentUnavailableView("QR Code", systemImage: "qrcode")
    }
}

// MARK: - ClipShareSheet

struct ClipShareSheet: View {
    let clip: Clip
    let episode: Episode
    let podcast: Podcast?
    var body: some View {
        ContentUnavailableView("Share Clip", systemImage: "scissors")
    }
}

// MARK: - Missing view stubs (T-podcast-ios-RESTART)

struct AgentNotesView: View {
    var spotlightTargetID: UUID? = nil
    var body: some View { ContentUnavailableView("Notes", systemImage: "note.text") }
}

struct AgentMemoriesView: View {
    var spotlightTargetID: UUID? = nil
    var body: some View { ContentUnavailableView("Memories", systemImage: "brain") }
}

struct NetworkingSettingsView: View {
    var body: some View { ContentUnavailableView("Networking", systemImage: "network") }
}

struct NostrConversationDetailView: View {
    let conversation: NostrConversationRecord
    var body: some View { ContentUnavailableView("Conversation", systemImage: "message") }
}
