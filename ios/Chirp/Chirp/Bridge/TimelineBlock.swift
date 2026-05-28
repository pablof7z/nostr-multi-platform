import Foundation

// ─────────────────────────────────────────────────────────────────────────
// T146 — Swift mirror of `nmp_threading::TimelineBlock` + the per-event
// metadata `nmp_app_chirp` ships alongside the blocks.
//
// Wire shape produced by `projections["nmp.feed.home"]`:
//   { "blocks": [TimelineBlock], "cards": [ChirpEventCard], "page": ..., "metrics": ... }
//
// `TimelineBlock` is a tagged enum on the Rust side (serde default
// representation). The two variants are:
//   { "Standalone": { "id": "<event_id>", "root": ThreadPointer? } }
//   { "Module": { "events": [...], "has_gap": bool, "root": ThreadPointer? } }
//
// `Standalone.root` carries `#[serde(default, skip_serializing_if =
// "Option::is_none")]` on the Rust side, so the `root` field is ABSENT
// (not `null`) when the standalone event is itself a thread root. It is
// present only when the event is a reply that could not be stitched into a
// chain (partial-chain head).
//
// `ThreadPointer` is another tagged enum (Event / Address / External).
// Chirp only ever displays the Event variant's id (for the "show this
// thread" gap pill), so the others are decoded into a typed enum but the
// renderer treats them as anchor-only.
//
// `ChirpEventCard` is a flat decoder-free struct. Author display name and
// avatar URL come from the existing `KernelModel.items: [TimelineItem]`
// lookup on the Swift side (D1 placeholders already in place there), so
// the projection layer does not duplicate profile state.
// ─────────────────────────────────────────────────────────────────────────

/// One block in the modular home timeline. `standalone` renders as the
/// existing tweet row; `module` renders as a vertical-line stack of two or
/// three events sharing the same thread.
enum TimelineBlock: Decodable, Equatable {
    case standalone(eventID: String, root: ThreadPointer?)
    case module(events: [String], hasGap: Bool, root: ThreadPointer?)

    var stableID: String {
        switch self {
        case .standalone(let id, _):
            return "standalone:\(id)"
        case .module(let events, _, let root):
            return "module:\(root?.eventID ?? events.first ?? "unknown"):\(events.joined(separator: ","))"
        }
    }

    /// Display-order ids in this block. Standalone returns one id; module
    /// returns its `events` array (root-first newest-last).
    var eventIDs: [String] {
        switch self {
        case .standalone(let id, _): return [id]
        case .module(let events, _, _): return events
        }
    }

    /// True when the block is a module that the grouper flagged as having
    /// either a missing ancestor, a long lookback gap, or a mismatched
    /// declared root. Drives the "Show this thread" pill in the renderer.
    var hasGap: Bool {
        switch self {
        case .standalone: return false
        case .module(_, let hasGap, _): return hasGap
        }
    }

    // ── serde tagged-enum decoding ─────────────────────────────────────
    //
    // Rust's `#[derive(Serialize, Deserialize)]` default for an enum emits
    // `{ "Variant": payload }`. We probe both variants in order.

    private enum CodingKeys: String, CodingKey {
        case standalone = "Standalone"
        case module = "Module"
    }

    private struct StandalonePayload: Decodable {
        let id: String
        let root: ThreadPointer?
    }

    private struct ModulePayload: Decodable {
        let events: [String]
        let hasGap: Bool
        let root: ThreadPointer?
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let standalone = try container.decodeIfPresent(StandalonePayload.self, forKey: .standalone) {
            self = .standalone(eventID: standalone.id, root: standalone.root)
            return
        }
        if let module = try container.decodeIfPresent(ModulePayload.self, forKey: .module) {
            self = .module(events: module.events, hasGap: module.hasGap, root: module.root)
            return
        }
        throw DecodingError.dataCorrupted(
            DecodingError.Context(codingPath: decoder.codingPath,
                                  debugDescription: "unknown TimelineBlock variant")
        )
    }
}

/// Anchor for a reply / comment chain. Only the `event` variant carries a
/// renderable id; the others terminate ancestor walks and are surfaced
/// only when the renderer needs to decide whether to show the "show this
/// thread" pill (`root != nil && root.event.id != top of module`).
enum ThreadPointer: Decodable, Equatable {
    case event(id: String, relay: String?, kind: UInt32?)
    case address(coord: String, relay: String?, kind: UInt32?)
    case external(uri: String)

    /// Event id if this pointer names a specific event; nil for address /
    /// external pointers (those terminate ancestor walks).
    var eventID: String? {
        if case .event(let id, _, _) = self { return id }
        return nil
    }

    private enum CodingKeys: String, CodingKey {
        case event = "Event"
        case address = "Address"
        case external = "External"
    }

    private struct EventPayload: Decodable {
        let id: String
        let relay: String?
        let kind: UInt32?
    }

    private struct AddressPayload: Decodable {
        let coord: String
        let relay: String?
        let kind: UInt32?
    }

    private struct ExternalPayload: Decodable {
        let uri: String
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let p = try container.decodeIfPresent(EventPayload.self, forKey: .event) {
            self = .event(id: p.id, relay: p.relay, kind: p.kind)
            return
        }
        if let p = try container.decodeIfPresent(AddressPayload.self, forKey: .address) {
            self = .address(coord: p.coord, relay: p.relay, kind: p.kind)
            return
        }
        if let p = try container.decodeIfPresent(ExternalPayload.self, forKey: .external) {
            self = .external(uri: p.uri)
            return
        }
        throw DecodingError.dataCorrupted(
            DecodingError.Context(codingPath: decoder.codingPath,
                                  debugDescription: "unknown ThreadPointer variant")
        )
    }
}

/// Per-event render metadata. Author display name / avatar come from
/// `KernelModel.items: [TimelineItem]` — this struct is the minimal extra
/// payload `nmp-app-chirp` ships so blocks are self-renderable when an id
/// is not in the kernel's visible-items window (e.g., an ancestor that
/// arrived before its child took the row).
/// ADR-0032: Rust ships raw protocol data only. Display fields are
/// derived by the presentation layer:
///   • `createdAtDisplay`        → `createdAt.relativeTimeFromUnixSeconds`
///   • `authorAvatarInitials`    → `authorPubkey.displayInitials`
///   • `authorAvatarColor`       → `authorPubkey.pubkeyColorHex`
///   • `authorPubkeyShort`/`shortId` → `authorPubkey.shortHex` / `id.shortHex`
///
/// `authorDisplayName` and `authorPictureUrl` are `Optional<String>` —
/// `nil` when no kind:0 has arrived. View code falls back via
/// `authorPubkey.shortHex` / identicon URI.
struct ChirpEventCard: Decodable, Equatable, Identifiable, Sendable {
    let id: String
    let authorPubkey: String
    let kind: UInt32
    let createdAt: UInt64
    let content: String
    let contentTree: ContentTreeWire?
    let relationCounts: NoteRelationCounts?
    /// Flat mirror of `author_display.name` for renderers that want a
    /// simple display-name field without decoding the nested
    /// `AuthorDisplay` object. `nil` when no kind:0 has arrived.
    let authorDisplayName: String?
    /// Author's profile picture URL from kind:0. `nil` when no kind:0 has
    /// arrived or the metadata omits `picture` — presentation layer
    /// chooses a placeholder strategy.
    let authorPictureUrl: String?
    /// First 180 Unicode scalars of `content`, no ellipsis. Used by the
    /// `syntheticItem` builder in `ModularBlockView`.
    let contentPreview: String

    private enum CodingKeys: String, CodingKey {
        case id
        case authorPubkey
        case kind
        case createdAt
        case content
        case contentTree
        case relationCounts
        case authorDisplayName
        case authorPictureUrl
        case contentPreview
    }
}

struct NoteRelationCounts: Decodable, Equatable, Sendable {
    let replies: RelationCount
    let reactions: RelationCount
    let reposts: RelationCount
    let zaps: RelationCount
}

enum RelationCount: Decodable, Equatable, Sendable {
    case known(UInt64)
    case loading

    var value: UInt64? {
        if case .known(let count) = self { return count }
        return nil
    }

    private enum CodingKeys: String, CodingKey {
        case state
        case count
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if try container.decode(String.self, forKey: .state) == "known" {
            self = .known(try container.decode(UInt64.self, forKey: .count))
        } else {
            self = .loading
        }
    }
}

struct TimelineWindowCursor: Decodable, Equatable, Sendable {
    let createdAt: UInt64
    let id: String

    private enum CodingKeys: String, CodingKey {
        case createdAt
        case id
    }
}

struct TimelineWindowPage: Decodable, Equatable, Sendable {
    let limit: UInt
    let nextCursor: TimelineWindowCursor?
    let hasMore: Bool
    let totalBlocks: UInt

    private enum CodingKeys: String, CodingKey {
        case limit
        case nextCursor
        case hasMore
        case totalBlocks
    }
}

/// Decoded `nmp-feed` home projection payload.
struct ChirpTimelineSnapshot: Decodable, Equatable {
    let blocks: [TimelineBlock]
    let cards: [ChirpEventCard]
    let page: TimelineWindowPage?

    static let empty = ChirpTimelineSnapshot(blocks: [], cards: [], page: nil)
}

// ─── nmp-content ContentTreeWire mirror ─────────────────────────────────
//
// M16-C7: Chirp now uses the registry types from
// ios/Chirp/Chirp/Components/NostrContent/ directly.  The hand-rolled
// ContentTreeWire, ContentWireNode, MediaKind, and WireNostrUri definitions
// have been replaced with their registry counterparts:
//
//   ContentTreeWire  →  public struct ContentTreeWire  (ContentTreeWire.swift)
//   ContentWireNode  →  public enum   NostrWireNode     (ContentTreeWire.swift)
//   MediaKind        →  public enum   NostrMediaKind    (ContentTreeWire.swift)
//   WireNostrUri     →  public struct NostrWireUri      (ContentTreeWire.swift)
//
// Type aliases below keep existing call-sites compiling without a rename sweep.

typealias ContentWireNode = NostrWireNode
typealias MediaKind = NostrMediaKind
typealias WireNostrUri = NostrWireUri

struct MentionProfile: Equatable, Sendable {
    let display: String
    let pictureUrl: String?
    let initials: String
    let colorHex: String
}
