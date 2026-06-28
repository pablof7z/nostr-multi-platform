import Foundation

// ─────────────────────────────────────────────────────────────────────────
// T146 — Swift mirror of `nmp_threading::TimelineBlock` + the per-event
// metadata `nmp_app_chirp` ships alongside the blocks.
//
// V-80 rung 7 NOTE: the HOME feed (`projections["nmp.feed.home"]`) no longer
// uses the `{ blocks, cards }` modular shape — it is now the OP-centric
// `OpFeedSnapshot` (`{ cards: [ChirpRootCard], page }`) defined lower
// in this file. The `TimelineBlock` enum + `ChirpEventCard` below are STILL
// used by the author-view / thread-view modular renderers
// (`ModularBlockView`), which keep the `{ blocks, cards }` shape — so they are
// retained unchanged.
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
// avatar URL are resolved through the kernel's profile projections when
// available, falling back to a synthetic item built from the card itself (D1
// placeholders already in place there), so the feed layer does not duplicate
// profile state.
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

/// Per-event render metadata. This is the minimal extra payload
/// `nmp-app-chirp` ships so flat-feed blocks are self-renderable before richer
/// profile data lands in the profile projections.
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
    /// Raw relay URLs that delivered this event, in store provenance order.
    let relayProvenance: [String]
    /// Typed NIP-18 repost signal. `true` when the kernel surfaced this card
    /// because a kind:6 repost superseded the original note — i.e. the Rust
    /// `TimelineEventCard.reposted_by` (FB `reposted_by` table) is present.
    ///
    /// This must NOT be re-derived as `kind == 6` in the view: for the
    /// OP-centric feed the card's `kind` is the *original* note's kind (the
    /// engine sources the body from the inner/target note and only stamps
    /// `reposted_by` + the repost timestamp), so a repost card carries
    /// `kind == 1` with `isRepost == true`. The kernel owns repost semantics
    /// (D0); the view reads this typed flag.
    let isRepost: Bool

    /// Memberwise init used by the typed FlatBuffers decoder
    /// (`TypedHomeFeedDecoder.makeCard`), which is the authoritative path.
    init(
        id: String,
        authorPubkey: String,
        kind: UInt32,
        createdAt: UInt64,
        content: String,
        contentTree: ContentTreeWire?,
        relationCounts: NoteRelationCounts?,
        authorDisplayName: String?,
        authorPictureUrl: String?,
        contentPreview: String,
        relayProvenance: [String],
        isRepost: Bool
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.kind = kind
        self.createdAt = createdAt
        self.content = content
        self.contentTree = contentTree
        self.relationCounts = relationCounts
        self.authorDisplayName = authorDisplayName
        self.authorPictureUrl = authorPictureUrl
        self.contentPreview = contentPreview
        self.relayProvenance = relayProvenance
        self.isRepost = isRepost
    }

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
        case relayProvenance
        case repostedBy
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        authorPubkey = try c.decode(String.self, forKey: .authorPubkey)
        kind = try c.decode(UInt32.self, forKey: .kind)
        createdAt = try c.decode(UInt64.self, forKey: .createdAt)
        content = try c.decode(String.self, forKey: .content)
        contentTree = try c.decodeIfPresent(ContentTreeWire.self, forKey: .contentTree)
        relationCounts = try c.decodeIfPresent(NoteRelationCounts.self, forKey: .relationCounts)
        authorDisplayName = try c.decodeIfPresent(String.self, forKey: .authorDisplayName)
        authorPictureUrl = try c.decodeIfPresent(String.self, forKey: .authorPictureUrl)
        contentPreview = try c.decode(String.self, forKey: .contentPreview)
        relayProvenance = try c.decodeIfPresent([String].self, forKey: .relayProvenance) ?? []
        // The Rust serde wire uses `#[serde(skip_serializing_if =
        // "Option::is_none")]` on `reposted_by`, so the key is ABSENT for a
        // plain note and a non-null object for a repost. Presence == repost.
        isRepost = c.contains(.repostedBy)
            && !((try? c.decodeNil(forKey: .repostedBy)) ?? false)
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

// ─────────────────────────────────────────────────────────────────────────
// V-80 rung 7 — OP-centric home feed.
//
// `projections["nmp.feed.home"]` is the Rust `RootFeedSnapshot<
// TimelineEventCard, Nip10ReplyAttribution>` (Rust alias: `OpFeedSnapshot`).
// Wire shape (after `.convertFromSnakeCase`):
//
//   { "cards": [{ "card": ChirpEventCard, "attribution": [ChirpReplyAttribution] }],
//     "page": TimelineWindowPage?, "metrics": null }
//
// The feed is thread-ROOTS-only: every entry is one root. A followed user's
// reply to a non-followed author's note surfaces THAT note here, tagged with
// the replier in `attribution`. Replies never get their own row.
//
// The generated `SnapshotProjections.homeFeed` binding uses the framework type
// name `OpFeedSnapshot` (issue #1613).
// ─────────────────────────────────────────────────────────────────────────

/// Raw attribution for one follow's reply to a feed root (mirror of Rust
/// `nmp_nip01::op_feed::Nip10ReplyAttribution`). Display fields fall back the
/// same way `ChirpEventCard` does: `authorDisplayName` is `nil` until the
/// author's kind:0 arrives — the view formats the raw pubkey meanwhile.
struct ChirpReplyAttribution: Decodable, Equatable, Identifiable, Sendable {
    let authorPubkey: String
    /// Flat mirror of `author_display.name`. `nil` until a kind:0 arrives.
    let authorDisplayName: String?
    /// Author's kind:0 picture URL. `nil` until a kind:0 arrives / omits it.
    let authorPictureUrl: String?
    let replyEventId: String
    let replyCreatedAt: UInt64

    /// Stable identity for `ForEach` — the reply event id is unique per root.
    var id: String { replyEventId }

    private enum CodingKeys: String, CodingKey {
        case authorPubkey
        case authorDisplayName
        case authorPictureUrl
        case replyEventId
        case replyCreatedAt
    }
}

/// One feed row: a root render card plus its raw attribution list (mirror of
/// Rust `nmp_feed::RootCard<C, A>`). The `attribution` array carries ALL
/// repliers raw; the renderer chooses how many to show (Q1).
struct ChirpRootCard: Decodable, Equatable, Identifiable, Sendable {
    let card: ChirpEventCard
    let attribution: [ChirpReplyAttribution]

    /// Identity is the inner card's id (for reposts the engine forced this to
    /// the superseded target id, so it is stable across the wrapper/target
    /// pair).
    var id: String { card.id }

    private enum CodingKeys: String, CodingKey {
        case card
        case attribution
    }
}

/// Decoded OP-centric home projection payload (`RootFeedSnapshot`).
///
/// The framework/protocol name — mirrors the Rust `nmp_nip01::op_feed::OpFeedSnapshot`
/// type alias (`RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>`). The
/// `nmp-codegen` registry uses this name in the generated `SnapshotProjections.homeFeed`
/// binding (issue #1613 — no app names in generic codegen tool).
struct OpFeedSnapshot: Decodable, Equatable {
    let cards: [ChirpRootCard]
    let page: TimelineWindowPage?

    static let empty = OpFeedSnapshot(cards: [], page: nil)

    private enum CodingKeys: String, CodingKey {
        case cards
        case page
        // `metrics` is present in the Rust shape but the engine always emits
        // `null`; we do not decode it (D1 forward-compat tolerates extra keys).
    }
}

// ─── nmp-content ContentTreeWire mirror ─────────────────────────────────
//
// M16-C7: Chirp now uses the registry types from
// apps/chirp/ios/Chirp/Components/NostrContent/ directly.  The hand-rolled
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

// ADR-0063 Lane E (#1671): `MentionProfile` (the component-facing whole-map
// profile struct) is removed. Inline mention / author labels read the per-key
// `keyedRefCache` (`refs.profile`) directly — no whole-map dictionary.
