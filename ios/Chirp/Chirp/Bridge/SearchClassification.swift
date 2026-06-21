import Foundation

/// Typed result of the Rust go-to-box classifier (`nmp_app_search_classify`).
///
/// Chirp is a thin shell: ALL query parsing (is it a bech32 entity? a hashtag?
/// a NIP-05 identifier? free text?) lives in Rust. This enum is the decoded
/// shape of that decision — the UI only switches on the case, it never parses.
///
/// Mirrors the wire JSON, which is internally tagged by `"kind"`:
///   profile · event · hashtag · nip05 · search · unsupported
enum SearchClassification: Decodable, Equatable {
    /// `npub` / `nprofile` → open this profile.
    case profile(pubkey: String, relays: [String])
    /// `note` / `nevent` → open this event's thread.
    case event(eventID: String, relays: [String], author: String?, kind: UInt32?)
    /// `#tag` or a bare token → open this hashtag feed. `tag` is normalized.
    case hashtag(tag: String)
    /// `name@domain` → resolve + open a profile (resolution not yet wired).
    case nip05(identifier: String)
    /// Free text → NIP-50 full-text search (not yet wired).
    case search(query: String)
    /// Recognized but unrouteable (naddr/nsec/empty). `reason` is diagnostic.
    case unsupported(reason: String)

    private enum CodingKeys: String, CodingKey {
        case kind
        case pubkey
        case relays
        case eventID = "event_id"
        case author
        case eventKind = "event_kind"
        case tag
        case identifier
        case query
        case reason
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try c.decode(String.self, forKey: .kind)
        switch kind {
        case "profile":
            self = .profile(
                pubkey: try c.decode(String.self, forKey: .pubkey),
                relays: try c.decodeIfPresent([String].self, forKey: .relays) ?? [])
        case "event":
            self = .event(
                eventID: try c.decode(String.self, forKey: .eventID),
                relays: try c.decodeIfPresent([String].self, forKey: .relays) ?? [],
                author: try c.decodeIfPresent(String.self, forKey: .author),
                kind: try c.decodeIfPresent(UInt32.self, forKey: .eventKind))
        case "hashtag":
            self = .hashtag(tag: try c.decode(String.self, forKey: .tag))
        case "nip05":
            self = .nip05(identifier: try c.decode(String.self, forKey: .identifier))
        case "search":
            self = .search(query: try c.decode(String.self, forKey: .query))
        case "unsupported":
            self = .unsupported(
                reason: try c.decodeIfPresent(String.self, forKey: .reason) ?? "unsupported")
        default:
            self = .unsupported(reason: "unknown kind: \(kind)")
        }
    }
}
