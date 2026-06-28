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

/// ADR-0063 Lane E (#1671) — per-key observable bridge for a view that renders
/// a SET of pubkeys in a single SwiftUI node that cannot be split into
/// per-pubkey leaves.
///
/// Inline profile mentions render as one concatenated `Text` run (no per-mention
/// SwiftUI view), so a `NostrProfileName`-style single-key observer can't be
/// hosted per mention. A note's content view holds one of these as a
/// `@StateObject` and calls `observe(_:pubkeys:)` with exactly the mention
/// pubkeys it renders. The observer subscribes to `refs.profile` row changes
/// FILTERED to that set, and fires `objectWillChange` only when one of THOSE
/// rows commits — re-evaluating only this note's content (which re-reads each
/// mention label from the `KeyedRefCache`), never the whole map and never a
/// broad `@Published` invalidation of `KernelModel`.
///
/// Holds NO profile data — only the subscription + the observed key set (the
/// `KeyedRefCache` is the source, D4).
@MainActor
final class KeyedRefMultiRowObserver: ObservableObject {
    private var cancellable: AnyCancellable?
    /// The key set currently observed. Re-`observe` with an equal set is a
    /// no-op (idempotent across body re-evaluations / `.task(id:)` re-fires).
    private var observedKeys: Set<String> = []

    /// Subscribe to `publisher` (the host's `profileRowChanged`), filtered so
    /// only a `refs.profile` row whose `rowKey` is in `pubkeys` triggers
    /// `objectWillChange`. Idempotent for an unchanged set; a changed set
    /// re-points the subscription. An empty set tears the subscription down.
    func observe(_ publisher: AnyPublisher<KeyedRowChange, Never>, pubkeys: Set<String>) {
        guard observedKeys != pubkeys else { return }
        observedKeys = pubkeys
        guard !pubkeys.isEmpty else {
            cancellable = nil
            return
        }
        cancellable = publisher
            .filter { $0.projectionKey == "refs.profile" && pubkeys.contains($0.rowKey) }
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                self?.objectWillChange.send()
            }
    }
}
