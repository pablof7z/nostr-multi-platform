import Foundation

// Small shell-side interaction DTOs that are NOT part of any action-intent
// wire path. Extracted from the retired `ChirpActionSpecBridge.swift` when the
// `ChirpActionIntent` JSON social-intent lane was deleted (M14-1 PR2 / #2145):
// social writes now ride the generated `GeneratedActionBuilders` byte builders,
// but these two presentation/scoping types are still referenced by the views
// and the feed bridge, so they live here.

/// Interest scope for a feed/observed-interest open. Mirrors the Rust
/// `InterestScope` enum ordinal passed across the C-ABI feed doorway.
enum InterestScope: UInt32 {
    case activeAccount = 0
    case global = 1
}

/// A compose-time reply target: the note a new kind:1 will reply to. Carried by
/// the composer / thread views into `KernelHandle.publishNote(content:replyTo:)`,
/// which forwards only the parent event id to the generated `publishReply`
/// builder — Rust derives the NIP-10 tags from the STORED parent event.
struct ChirpReplyTarget: Codable, Equatable, Identifiable {
    let eventID: String
    let authorPubkey: String
    let createdAt: UInt64
    let content: String

    var id: String { eventID }

    init(eventID: String, authorPubkey: String, createdAt: UInt64 = 0, content: String = "") {
        self.eventID = eventID
        self.authorPubkey = authorPubkey
        self.createdAt = createdAt
        self.content = content
    }

    init(row: NoteRowModel) {
        self.init(
            eventID: row.id,
            authorPubkey: row.authorPubkey,
            createdAt: row.createdAt,
            content: row.content
        )
    }

    private enum CodingKeys: String, CodingKey {
        case eventID = "event_id"
        case authorPubkey = "author_pubkey"
        case createdAt = "created_at"
        case content
    }
}
