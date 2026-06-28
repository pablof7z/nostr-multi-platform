import Combine
import SwiftUI

/// Host bridge for profile projections owned by the NMP kernel.
///
/// Registry components call this bridge with stable Nostr references. The app
/// supplies the platform adapter; the component owns when to claim, release,
/// and re-read the current projection.
///
/// ADR-0063 Lane E (#1671): the claim/read surface is the unified
/// typed profile-ref FFI adapters + `refs.profile` typed per-key accessor.
/// `resolveProfile` / `releaseProfile` wrap the typed adapters;
/// `profileCard(forPubkey:)` reads the
/// decoded `ProfileCard` from the per-key `KeyedRefCache` (the SOURCE — no
/// app-side cache, D4); and `profileRowChanged` is the per-key Combine signal a
/// leaf binds, filtered on its pubkey, so EXACTLY ONE avatar/name re-renders
/// when that one pubkey's kind:0 arrives.
@MainActor
protocol NostrProfileHost: AnyObject {
    /// The decoded `ProfileCard` for `pubkey` from the keyed-ref cache, or `nil`
    /// when no row is cached yet (the resolve is in flight). The unified read
    /// backed by `refs.profile` (ADR-0063 Lane E).
    func profileCard(forPubkey pubkey: String) -> ProfileCard?
    /// Resolve `pubkey`'s kind:0 via the unified `resolve_ref` seam at an
    /// explicit `shape` + `liveness`. Feed avatars / inline list contexts /
    /// search / notifications pass `.profileRef` + `.cacheOk` (serve from
    /// cache, OneShot fill, no live sub); the open profile screen passes
    /// `.profileCard` + `.live` (a tailing sub so profile-edit updates flow in).
    func resolveProfile(
        pubkey: String, consumerID: String, shape: RefShape, liveness: RefLiveness)
    /// Release a `resolveProfile` interest. Pass the SAME `pubkey` / `consumerID`.
    func releaseProfile(pubkey: String, consumerID: String)
    /// Per-key row-change publisher. A leaf subscribes filtered on its
    /// `rowKey == pubkey` (and `projectionKey == "refs.profile"`) so only that
    /// one leaf re-renders when the pubkey's row commits/clears.
    var profileRowChanged: AnyPublisher<KeyedRowChange, Never> { get }
}

extension NostrProfileHost {
    /// Convenience for the common list/inline path: resolve the lightweight
    /// `profile.ref` shape with `.cacheOk` liveness. Registry leaves (avatar,
    /// name) in a list/reading context call this; the profile screen calls the
    /// explicit `shape:liveness:` form with `.profileCard` + `.live`.
    func resolveProfile(pubkey: String, consumerID: String) {
        resolveProfile(
            pubkey: pubkey, consumerID: consumerID, shape: .profileRef, liveness: .cacheOk)
    }

    /// Convenience read returning the presentation `ProfileWire` for `pubkey`,
    /// derived from the keyed-ref `ProfileCard`. `nil` when no row is cached.
    /// `ProfileWire.npub` is `nil` (ADR-0032 / V-115 — bech32 is encoded
    /// host-side on demand). The single place the shell maps a keyed-ref card to
    /// the leaf-facing wire type.
    func profile(forPubkey pubkey: String) -> ProfileWire? {
        guard let card = profileCard(forPubkey: pubkey) else { return nil }
        return ProfileWire(
            pubkey: pubkey,
            displayName: (card.displayName?.isEmpty == false) ? card.displayName : nil,
            about: card.about.isEmpty ? nil : card.about,
            pictureUrl: card.pictureUrl,
            nip05: card.nip05.isEmpty ? nil : card.nip05,
            npub: nil,
            npubShort: pubkey.shortHex)
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
