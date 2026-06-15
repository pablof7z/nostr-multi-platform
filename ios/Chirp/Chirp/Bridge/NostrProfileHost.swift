import SwiftUI

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Registry components call this bridge with stable Nostr references. The app
/// supplies the platform adapter; the component owns when to claim, release,
/// and re-read the current projection.
@MainActor
protocol NostrProfileHost: AnyObject {
    func profile(forPubkey pubkey: String) -> ProfileWire?
    /// Claim `pubkey`'s kind:0 with an explicit subscription shape. `.cacheOk`
    /// (feed avatars, inline list contexts) serves from cache with a OneShot
    /// fill and no live sub; `.live` (the profile screen) opens a Tailing
    /// interest so reactive profile-edit updates flow in.
    func claimProfile(pubkey: String, consumerID: String, liveness: ProfileLiveness)
    func releaseProfile(pubkey: String, consumerID: String)
}

extension NostrProfileHost {
    /// Convenience for the common list/inline path: claim cache-ok. Registry
    /// leaves (avatar, name) that have no reason to open a live subscription
    /// call this; the profile screen calls the explicit `liveness:` form.
    func claimProfile(pubkey: String, consumerID: String) {
        claimProfile(pubkey: pubkey, consumerID: consumerID, liveness: .cacheOk)
    }
}

private struct NostrProfileHostKey: EnvironmentKey {
    nonisolated(unsafe)
    static let defaultValue: NostrProfileHost? = nil
}

extension EnvironmentValues {
    var nostrProfileHost: NostrProfileHost? {
        get { self[NostrProfileHostKey.self] }
        set { self[NostrProfileHostKey.self] = newValue }
    }
}
