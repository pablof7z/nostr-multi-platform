import Foundation
import SwiftUI

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
    public let npub: String
    /// Rust-truncated npub (e.g. `npub1abcd…wxyz`). Display only.
    public let npubShort: String

    public init(
        pubkey: String,
        displayName: String? = nil,
        about: String? = nil,
        pictureUrl: String? = nil,
        nip05: String? = nil,
        npub: String,
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

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Registry components call this bridge with stable Nostr references. The app
/// supplies the platform adapter; the component owns when to claim, release,
/// and re-read the current projection.
@MainActor
public protocol NostrProfileHost: AnyObject {
    func profile(forPubkey pubkey: String) -> ProfileWire?
    func claimProfile(pubkey: String, consumerID: String)
    func releaseProfile(pubkey: String, consumerID: String)
}

private struct NostrProfileHostKey: EnvironmentKey {
    nonisolated(unsafe)
    static let defaultValue: NostrProfileHost? = nil
}

public extension EnvironmentValues {
    var nostrProfileHost: NostrProfileHost? {
        get { self[NostrProfileHostKey.self] }
        set { self[NostrProfileHostKey.self] = newValue }
    }
}
