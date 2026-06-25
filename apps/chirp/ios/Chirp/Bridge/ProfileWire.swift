import Foundation

/// Wire type for a Nostr user profile, sourced from the kernel snapshot.
/// Carries minimal fields needed for presentation; the kernel pre-formats
/// profile metadata from kind:0 events.
public struct ProfileWire: Equatable, Sendable {
    public let pubkey: String
    public let displayName: String?
    public let about: String?
    public let pictureUrl: String?
    public let nip05: String?
    /// Full bech32 `npub1…` string. Use for copy / share.
    /// `nil` when the profile originates from a mention projection that does
    /// not carry the bech32 encoding (callers that pass `npub` to a share
    /// sheet or clipboard must guard for nil).
    public let npub: String?
    /// Rust-truncated npub (e.g. `npub1abcd…wxyz`). Display only.
    public let npubShort: String

    public init(
        pubkey: String,
        displayName: String? = nil,
        about: String? = nil,
        pictureUrl: String? = nil,
        nip05: String? = nil,
        npub: String? = nil,
        npubShort: String
    ) {
        self.pubkey = pubkey
        self.displayName = displayName
        self.about = about
        self.pictureUrl = pictureUrl
        self.nip05 = nip05
        self.npub = npub
        self.npubShort = npubShort
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
}
