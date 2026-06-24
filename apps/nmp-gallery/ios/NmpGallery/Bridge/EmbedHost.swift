import Foundation
import Observation
import SwiftUI
import os.log

private let ehLog = Logger(subsystem: "org.nmp.gallery", category: "EmbedHost")

/// Gallery-side cache of the pre-resolved `claimed_event_embeds` projection
/// produced by `nmp-ffi`'s embed sidecar (issue #1283 / ADR-0034).
///
/// The renderer (`NostrContentView` / `EmbeddedEvent`) is frontend-driven
/// (ADR-0034 / M16): it walks a content tree, encounters an `EventRef(uri)`,
/// and the `EmbeddedEvent` view fires `sink.claim(uri, consumerId)` via
/// `EventClaimSinkProtocol`. The host (`KernelEventClaimSink`) decodes the
/// URI and forwards the raw event key through `resolve_ref`. The kernel
/// registers a `OneshotApi` interest, fetches via relays (or cache-hits),
/// and surfaces the resolved event in
/// `snapshot.projections.claimed_events[primary_id]`. `nmp-ffi` then
/// kind-dispatches each entry via `nmp_content::resolve_embed_projection`
/// and emits the pre-resolved `EmbeddedEventEnvelope` map under
/// `projections["claimed_event_embeds"]`. This class is the gallery's
/// read-side cache of that projection.
///
/// Each snapshot push calls `update(claimedEventEmbeds:)`; on the next redraw
/// the SwiftUI view tree re-reads `envelopeForURI(_:)` /
/// `envelopeForPrimaryID(_:)` and the registry dispatches to the right renderer.
///
/// Doctrine: D8 — no polling. Updates are push-driven by the snapshot
/// callback; SwiftUI invalidates dependent views via `@Observable`.
@MainActor
@Observable
final class EmbedHost {
    /// Claimed envelopes keyed by `primary_id` (event-id hex for nevent/note,
    /// `"kind:pubkey:d"` coordinate for naddr). Latest-snapshot-wins; rebuilt
    /// from the pre-resolved `claimed_event_embeds` projection on each non-nil
    /// push. The kind-dispatch runs in Rust (`nmp-content`); this class is
    /// decode-only.
    private(set) var envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [:]

    /// Diagnostics — number of resolved envelopes in the current snapshot.
    var count: Int { envelopesByPrimaryID.count }

    /// Called on every snapshot tick with the pre-resolved embed map from
    /// `projections["claimed_event_embeds"]`.  A nil or empty value leaves the
    /// previous state intact (stable, not flicker) — matches the one-tick-lag
    /// semantics of the Rust sidecar (D6: graceful degradation).
    func update(claimedEventEmbeds: [String: EmbeddedEventEnvelope]?) {
        guard let embeds = claimedEventEmbeds, !embeds.isEmpty else { return }
        envelopesByPrimaryID = embeds
    }

    /// Lookup an envelope by `primary_id`. Used by `EmbeddedEvent` after the
    /// renderer's URI → primary-id resolution.
    func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope? {
        envelopesByPrimaryID[id]
    }

    /// Lookup an envelope by the original `nostr:` URI. Tries the URI as a
    /// direct key, then walks the value set looking for a matching `uri`
    /// (rare — only when the snapshot used a different key than the
    /// renderer-side URI parse would).
    func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope? {
        if let direct = envelopesByPrimaryID[uri] {
            return direct
        }
        // Linear scan only on miss. Map is small (one entry per claimed embed).
        return envelopesByPrimaryID.values.first { $0.uri == uri }
    }
}

// MARK: - Environment wiring

private struct EmbedHostKey: EnvironmentKey {
    static let defaultValue: EmbedHost? = nil
}

private struct EmbedClaimSinkKey: EnvironmentKey {
    static let defaultValue: EventClaimSinkProtocol? = nil
}

private struct NostrKindRegistryKey: EnvironmentKey {
    // Optional default — the gallery shell installs a real registry via
    // `.environment(\.nostrKindRegistry, …)` in NmpGalleryApp. Keeping the
    // default optional sidesteps Swift 6's main-actor-isolation rule for
    // `EnvironmentKey.defaultValue` (it must be nonisolated, but
    // `NostrKindRegistry.makeDefault()` is `@MainActor`).
    static let defaultValue: NostrKindRegistry? = nil
}

extension EnvironmentValues {
    var embedHost: EmbedHost? {
        get { self[EmbedHostKey.self] }
        set { self[EmbedHostKey.self] = newValue }
    }

    var embedClaimSink: EventClaimSinkProtocol? {
        get { self[EmbedClaimSinkKey.self] }
        set { self[EmbedClaimSinkKey.self] = newValue }
    }

    var nostrKindRegistry: NostrKindRegistry? {
        get { self[NostrKindRegistryKey.self] }
        set { self[NostrKindRegistryKey.self] = newValue }
    }
}
