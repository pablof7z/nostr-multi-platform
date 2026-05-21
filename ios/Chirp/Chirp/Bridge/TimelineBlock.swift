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

    private enum CodingKeys: String, CodingKey {
        case id
        case authorPubkey = "author_pubkey"
        case kind
        case createdAt = "created_at"
        case content
        case contentTree = "content_tree"
    }
}

/// Decoded `nmp_app_chirp_snapshot` payload.
struct ChirpTimelineSnapshot: Decodable, Equatable {
    let blocks: [TimelineBlock]
    let cards: [ChirpEventCard]

    static let empty = ChirpTimelineSnapshot(blocks: [], cards: [])
}

// ─── nmp-content ContentTreeWire mirror ──────────────────────────────────

struct ContentTreeWire: Decodable, Equatable {
    let nodes: [ContentWireNode]
    let roots: [UInt32]
    let mode: String?
}

/// Typed mirror of the Rust `nmp_content::MediaKind` enum. Raw values match the
/// PascalCase Rust serde representation (the Rust enum has no `rename_all`).
/// Pinned cross-platform by `crates/nmp-content/src/wire/tests.rs`.
enum MediaKind: String, Decodable, Equatable {
    case image = "Image"
    case video = "Video"
    case audio = "Audio"
}

enum ContentWireNode: Decodable, Equatable {
    case text(String)
    case mention(WireNostrUri)
    case eventRef(WireNostrUri)
    case hashtag(String)
    case url(String)
    case media(urls: [String], mediaKind: MediaKind)
    case emoji(shortcode: String, url: String?)
    case paragraph(children: [UInt32])
    case heading(level: UInt8, children: [UInt32])
    case emphasis(children: [UInt32])
    case strong(children: [UInt32])
    case inlineCode(String)
    case softBreak
    case hardBreak
    case placeholder

    private enum CodingKeys: String, CodingKey {
        case kind, text, uri, tag, url, urls, mediaKind = "media_kind"
        case shortcode, level, children, code
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "text":
            self = .text(try c.decode(String.self, forKey: .text))
        case "mention":
            self = .mention(try c.decode(WireNostrUri.self, forKey: .uri))
        case "event_ref":
            self = .eventRef(try c.decode(WireNostrUri.self, forKey: .uri))
        case "hashtag":
            self = .hashtag(try c.decode(String.self, forKey: .tag))
        case "url":
            self = .url(try c.decode(String.self, forKey: .url))
        case "media":
            self = .media(
                urls: try c.decode([String].self, forKey: .urls),
                mediaKind: try c.decode(MediaKind.self, forKey: .mediaKind)
            )
        case "emoji":
            self = .emoji(
                shortcode: try c.decode(String.self, forKey: .shortcode),
                url: try c.decodeIfPresent(String.self, forKey: .url)
            )
        case "paragraph":
            self = .paragraph(children: try c.decode([UInt32].self, forKey: .children))
        case "heading":
            self = .heading(
                level: try c.decode(UInt8.self, forKey: .level),
                children: try c.decode([UInt32].self, forKey: .children)
            )
        case "emphasis":
            self = .emphasis(children: try c.decode([UInt32].self, forKey: .children))
        case "strong":
            self = .strong(children: try c.decode([UInt32].self, forKey: .children))
        case "inline_code":
            self = .inlineCode(try c.decode(String.self, forKey: .code))
        case "soft_break":
            self = .softBreak
        case "hard_break":
            self = .hardBreak
        default:
            self = .placeholder
        }
    }
}

struct WireNostrUri: Decodable, Equatable {
    let uri: String
    let kind: String
    let primaryId: String
    let relays: [String]
    let author: String?
    let eventKind: UInt32?

    private enum CodingKeys: String, CodingKey {
        case uri, kind, relays, author
        case primaryId = "primary_id"
        case eventKind = "event_kind"
    }
}

struct MentionProfile: Equatable {
    let display: String
    let pictureUrl: String?
    let initials: String
    let colorHex: String
}
