import Foundation

// ─────────────────────────────────────────────────────────────────────────
// T146 — Swift mirror of `nmp_threading::TimelineBlock` + the per-event
// metadata `nmp_app_chirp` ships alongside the blocks.
//
// Wire shape produced by `nmp_app_chirp_snapshot(handle)`:
//   { "blocks": [TimelineBlock], "cards": [ChirpEventCard] }
//
// `TimelineBlock` is a tagged enum on the Rust side (serde default
// representation). The two variants are:
//   { "Standalone": "<event_id>" }
//   { "Module": { "events": [...], "has_gap": bool, "root": ThreadPointer? } }
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
    case standalone(eventID: String)
    case module(events: [String], hasGap: Bool, root: ThreadPointer?)

    var stableID: String {
        switch self {
        case .standalone(let id):
            return "standalone:\(id)"
        case .module(let events, _, let root):
            return "module:\(root?.eventID ?? events.first ?? "unknown"):\(events.joined(separator: ","))"
        }
    }

    /// Display-order ids in this block. Standalone returns one id; module
    /// returns its `events` array (root-first newest-last).
    var eventIDs: [String] {
        switch self {
        case .standalone(let id): return [id]
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

    private struct ModulePayload: Decodable {
        let events: [String]
        let has_gap: Bool
        let root: ThreadPointer?
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let id = try container.decodeIfPresent(String.self, forKey: .standalone) {
            self = .standalone(eventID: id)
            return
        }
        if let module = try container.decodeIfPresent(ModulePayload.self, forKey: .module) {
            self = .module(events: module.events, hasGap: module.has_gap, root: module.root)
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
struct ChirpEventCard: Decodable, Equatable, Identifiable {
    let id: String
    let authorPubkey: String
    let kind: UInt32
    let createdAt: UInt64
    let content: String
    let contentTree: ContentTreeWire?
    /// V-27 thin-shell: relative "Xs ago" string computed in Rust at
    /// snapshot construction. Replaces the iOS `relativeTime(card:)` helper.
    let createdAtDisplay: String
    /// V-27 thin-shell: two-char uppercase avatar initials from
    /// `author_pubkey`. Replaces the iOS `defaultInitials(pubkey:)` helper.
    let authorAvatarInitials: String
    /// V-27 thin-shell: deterministic 6-hex avatar background colour
    /// (uppercase, no `#`). Same djb2 algorithm as DM and NIP-29 surfaces so
    /// every author renders with the same tint across the app. Replaces the
    /// iOS `defaultColor(pubkey:)` helper (which used a different algorithm
    /// and produced inconsistent tints across surfaces).
    let authorAvatarColor: String
    /// V-27 thin-shell: abbreviated hex pubkey (`<first 8>…<last 8>`) for
    /// the Twitter-style secondary-identifier slot. Replaces the iOS
    /// `displayPubkey(item:card:)` helper (which used 6/4 abbreviation —
    /// the move to the cross-surface 8/8 algorithm shifts the abbreviation
    /// by two characters; deliberate consistency fix, not a regression).
    let authorPubkeyShort: String
    /// V-27 thin-shell: flat mirror of `author_display.name` so Swift can
    /// bind a single string without decoding the nested `AuthorDisplay`
    /// struct. Used by `syntheticItem` as the display-name fallback.
    let authorDisplayName: String
    /// V-28 thin-shell: abbreviated event id (`<first 8>…<last 8>`) computed
    /// in Rust. Used by `syntheticItem` to populate `TimelineItem.shortId`
    /// without slicing the raw 64-char `id` in Swift (aim.md §6.9).
    let shortId: String
    /// V-32 thin-shell: author's profile picture URL — either the parsed
    /// kind:0 `picture` field or the `identicon:<first 16-hex>` placeholder
    /// from `nmp_core::substrate::picture_placeholder`. Replaces the
    /// `identicon:\(card.authorPubkey.prefix(8))` string interpolation in
    /// `ModularBlockView.swift`'s `syntheticItem` builder (the prefix shifts
    /// from 8→16 hex chars — deliberate alignment with the cross-surface
    /// `picture_placeholder` algorithm, NOT a regression).
    let authorPictureUrl: String
    /// V-32 thin-shell: first 180 Unicode scalars of `content`, no ellipsis.
    /// Replaces the `String(card.content.prefix(180))` call-site in
    /// `ModularBlockView.swift`'s `syntheticItem` builder.
    let contentPreview: String

    private enum CodingKeys: String, CodingKey {
        case id
        case authorPubkey = "author_pubkey"
        case kind
        case createdAt = "created_at"
        case content
        case contentTree = "content_tree"
        case createdAtDisplay = "created_at_display"
        case authorAvatarInitials = "author_avatar_initials"
        case authorAvatarColor = "author_avatar_color"
        case authorPubkeyShort = "author_pubkey_short"
        case authorDisplayName = "author_display_name"
        case shortId = "short_id"
        case authorPictureUrl = "author_picture_url"
        case contentPreview = "content_preview"
    }
}

/// Decoded `nmp_app_chirp_snapshot` payload.
struct ChirpTimelineSnapshot: Decodable, Equatable {
    let blocks: [TimelineBlock]
    let cards: [ChirpEventCard]

    static let empty = ChirpTimelineSnapshot(blocks: [], cards: [])
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

struct MentionProfile: Equatable {
    let display: String
    let pictureUrl: String?
    let initials: String
    let colorHex: String
}
