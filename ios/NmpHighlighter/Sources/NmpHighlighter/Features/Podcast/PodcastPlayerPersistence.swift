import Foundation

struct PositionRecord: Codable {
    var guid: String
    var position: Double
    var lastPlayedAt: Date
    /// Minimal snapshot for cold-launch rehydration so the MiniPlayer can show
    /// the last episode (paused) without waiting on relay sync. Once the user
    /// taps play, we still go through `load(artifact:)` to wire AVPlayer.
    var snapshot: ArtifactSnapshot?
}

struct ChapterSnapshot: Codable {
    var startSeconds: Double
    var title: String
}

struct ArtifactSnapshot: Codable {
    var title: String
    var image: String
    var podcastShowTitle: String
    var podcastItemGuid: String
    var podcastGuid: String
    var audioUrl: String
    var audioPreviewUrl: String
    var transcriptUrl: String
    var durationSeconds: Int64?
    var groupId: String
    var shareEventId: String
    var pubkey: String
    var createdAt: UInt64?
    var note: String
    var chapters: [ChapterSnapshot]

    init(from record: ArtifactRecord) {
        self.title = record.preview.title
        self.image = record.preview.image
        self.podcastShowTitle = record.preview.podcastShowTitle
        self.podcastItemGuid = record.preview.podcastItemGuid
        self.podcastGuid = record.preview.podcastGuid
        self.audioUrl = record.preview.audioUrl
        self.audioPreviewUrl = record.preview.audioPreviewUrl
        self.transcriptUrl = record.preview.transcriptUrl
        self.durationSeconds = record.preview.durationSeconds
        self.groupId = record.groupId
        self.shareEventId = record.shareEventId
        self.pubkey = record.pubkey
        self.createdAt = record.createdAt
        self.note = record.note
        self.chapters = record.preview.chapters.map {
            ChapterSnapshot(startSeconds: $0.startSeconds, title: $0.title)
        }
    }

    func materialize() -> ArtifactRecord {
        let preview = ArtifactPreview(
            id: shareEventId,
            url: "",
            title: title,
            author: "",
            image: image,
            description: "",
            source: "podcast",
            domain: "",
            catalogId: podcastItemGuid.isEmpty ? podcastGuid : podcastItemGuid,
            catalogKind: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid")
                : "podcast:item:guid",
            podcastGuid: podcastGuid,
            podcastItemGuid: podcastItemGuid,
            podcastShowTitle: podcastShowTitle,
            audioUrl: audioUrl,
            audioPreviewUrl: audioPreviewUrl,
            transcriptUrl: transcriptUrl,
            feedUrl: "",
            publishedAt: "",
            durationSeconds: durationSeconds,
            referenceTagName: "i",
            referenceTagValue: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid:\(podcastGuid)")
                : "podcast:item:guid:\(podcastItemGuid)",
            referenceKind: podcastItemGuid.isEmpty
                ? (podcastGuid.isEmpty ? "" : "podcast:guid")
                : "podcast:item:guid",
            highlightTagName: "",
            highlightTagValue: "",
            highlightReferenceKey: "",
            chapters: chapters.map { Chapter(startSeconds: $0.startSeconds, title: $0.title) }
        )
        return ArtifactRecord(
            preview: preview,
            groupId: groupId,
            shareEventId: shareEventId,
            pubkey: pubkey,
            createdAt: createdAt,
            note: note
        )
    }
}

enum TranscriptAvailability {
    case loading, available, unavailable
}

enum PodcastPlayerError: Error, LocalizedError {
    case emptyResult

    var errorDescription: String? {
        switch self {
        case .emptyResult: return "No highlight returned from publish."
        }
    }
}
