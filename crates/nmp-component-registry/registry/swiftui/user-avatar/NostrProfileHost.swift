import SwiftUI

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Registry components call this bridge with stable Nostr references. The app
/// supplies one platform adapter that maps `resolveProfileRef` to the kernel's
/// profile `resolve_ref` path and reads the current row from `refs.profile`.
/// Components own when to resolve, release, and re-read the projection.
@MainActor
public protocol NostrProfileHost: AnyObject {
    func profile(forPubkey pubkey: String) -> ProfileWire?
    func resolveProfileRef(pubkey: String, consumerID: String)
    func releaseProfileRef(pubkey: String, consumerID: String)
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
