import Foundation

/// Wire type for a Nostr user profile, decoded from the `nmp-profile`
/// projection emitted by the kernel.
///
/// `npub` is Rust-formatted (the canonical bech32 NIP-19 encoder) — never
/// reformat it in Swift (aim.md §6.9). `npubShort` is NOT part of the wire
/// (#3098): abbreviation is pure string truncation of `npub`, a display
/// decision the host owns, so it is derived locally in [`npubShort`].
public struct ProfileWire: Codable, Equatable, Sendable {
    public let pubkey: String
    public let displayName: String?
    public let about: String?
    public let pictureUrl: String?
    public let nip05: String?
    /// Full bech32 `npub1…` string. Use for copy / share.
    public let npub: String

    public init(
        pubkey: String,
        displayName: String? = nil,
        about: String? = nil,
        pictureUrl: String? = nil,
        nip05: String? = nil,
        npub: String
    ) {
        self.pubkey = pubkey
        self.displayName = displayName
        self.about = about
        self.pictureUrl = pictureUrl
        self.nip05 = nip05
        self.npub = npub
    }

    /// Locally-truncated npub (e.g. `npub1abcd…wxyz`): first 10 chars + `"…"`
    /// + last 6 chars of `npub`, unchanged when already short enough to fit.
    /// Pure string truncation — never re-derives the bech32 encoding itself
    /// (that stays Rust-owned), mirrors the shape `nmp_core::display::short_npub`
    /// used to bake into this wire (#3098).
    public var npubShort: String {
        Self.truncateNpub(npub)
    }

    /// Stable display label: `displayName` if set, else `npubShort`.
    public var display: String {
        if let name = displayName, !name.isEmpty { return name }
        return npubShort
    }

    /// Parsed avatar URL; `nil` when no picture is set or URL is empty.
    public var avatarURL: URL? {
        guard let str = pictureUrl, !str.isEmpty else { return nil }
        return URL(string: str)
    }

    private static func truncateNpub(_ npub: String) -> String {
        let chars = Array(npub)
        guard chars.count > 17 else { return npub }
        let head = String(chars.prefix(10))
        let tail = String(chars.suffix(6))
        return "\(head)…\(tail)"
    }
}
