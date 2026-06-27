import Foundation

// Shell-side compose-input value types (M14-1 / PR2 #2145).
//
// The JSON intent DTO (`ChirpActionIntent`) + its Rust round-trip
// (`nmp_app_chirp_dispatch_intent_bytes` / `ChirpActionSpecEnvelope`) were
// RETIRED in M14-1 / PR2: every social write now goes through a generated
// `GeneratedActionBuilders.*` byte builder dispatched via `dispatchBytes`, with
// Rust owning all protocol-tag construction (thin-shell rule). What remains here
// are the two pure value types the host UI still passes as raw compose input:
// the interest scope discriminant and the reply-target carrier.

/// Interest-scope discriminant mirrored from the kernel's `InterestScope`
/// (`activeAccount` vs `global`). Passed to `openInterest` / `closeInterest`.
enum InterestScope: UInt32 {
    case activeAccount = 0
    case global = 1
}

/// A note the composer is replying to. Raw compose input only: Rust's
/// `nmp.nip01.publish_note` action module turns these fields into the NIP-10
/// marked-form root/reply `e`-tags + thread `p`-tags — the shell never builds a
/// tag.
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
