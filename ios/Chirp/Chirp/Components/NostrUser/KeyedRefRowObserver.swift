import Combine
import Foundation

/// ADR-0063 Lane E (#1671) — per-key observable bridge between the NMP-owned
/// `KeyedRefCache.rowChanged` Combine publisher and a single SwiftUI leaf.
///
/// A leaf that renders ONE pubkey (an avatar, an inline name) holds one of
/// these as a `@StateObject` and calls `observe(_:pubkey:)` on mount. The
/// observer subscribes to the host's `refs.profile` row-change stream FILTERED
/// to that one `rowKey == pubkey`, and fires `objectWillChange` only when that
/// specific row commits or clears. SwiftUI then re-evaluates EXACTLY that one
/// leaf's body — which re-reads `profileCard(forPubkey:)` from the cache — so a
/// single kind:0 arrival re-renders exactly one avatar, never the whole map and
/// never a broad `@Published` invalidation of `KernelModel`.
///
/// This is the acceptance mechanism for #1671 Lane E: per-key observable
/// avatars with no app-side profile cache (the `KeyedRefCache` is the source;
/// this object holds NO profile data, only the subscription + the observed key).
@MainActor
final class KeyedRefRowObserver: ObservableObject {
    /// The Combine subscription to the filtered row-change stream. Held so it
    /// stays alive for the observer's lifetime and is torn down on dealloc / a
    /// re-`observe` to a different key.
    private var cancellable: AnyCancellable?
    /// The key currently observed. Re-`observe` with the same key is a no-op
    /// (idempotent across body re-evaluations / `.task(id:)` re-fires).
    private var observedKey: String?

    /// Subscribe to `publisher` (the host's `profileRowChanged`), filtered so
    /// only a row whose `rowKey == pubkey` in the `refs.profile` projection
    /// triggers `objectWillChange`. Idempotent for an unchanged `pubkey`;
    /// switching `pubkey` re-points the subscription.
    func observe(_ publisher: AnyPublisher<KeyedRowChange, Never>, pubkey: String) {
        guard observedKey != pubkey else { return }
        observedKey = pubkey
        cancellable = publisher
            .filter { $0.projectionKey == "refs.profile" && $0.rowKey == pubkey }
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                // The cache already committed the new row before publishing;
                // tell SwiftUI to re-evaluate this one leaf so it re-reads the
                // fresh card. We carry no payload — the cache is the source (D4).
                self?.objectWillChange.send()
            }
    }
}
