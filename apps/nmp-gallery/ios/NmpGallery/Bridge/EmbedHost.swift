import Foundation
import Observation
import SwiftUI
import os.log

private let ehLog = Logger(subsystem: "org.nmp.gallery", category: "EmbedHost")

/// Gallery-side mirror of the resolved event-ref embed envelope map derived
/// from `refs.event` after `resolve_ref` (ADR-0063 / ADR-0034).
///
/// The renderer (`NostrContentView` / `EmbeddedEvent`) is frontend-driven
/// (ADR-0034 / M16): it walks a content tree, encounters an `EventRef(uri)`,
/// and the `EmbeddedEvent` view fires `sink.resolveEventRef(uri, consumerId)` via
/// `EventRefResolverProtocol`. The host (`KernelEventRefResolver`) decodes the
/// URI and forwards the raw event key through `resolve_ref`. The kernel
/// registers a `OneshotApi` interest, fetches via relays (or cache-hits),
/// and surfaces the resolved raw event row under `refs.event`. The gallery Rust
/// adapter merges that row-delta store, kind-dispatches each entry via
/// `nmp_content::resolve_embed_projection`, and emits the derived
/// `EmbeddedEventEnvelope` map under `projections["refs.event.envelopes"]`.
/// This class is the gallery's read-side mirror of that materialised map.
///
/// Each snapshot push calls `update(resolvedEventEmbeds:)`; on the next redraw
/// the SwiftUI view tree re-reads `envelopeForURI(_:)` /
/// `envelopeForPrimaryID(_:)` and the registry dispatches to the right renderer.
///
/// Doctrine: D8 — no polling. Updates are push-driven by the snapshot
/// callback; SwiftUI invalidates dependent views via `@Observable`.
@MainActor
@Observable
final class EmbedHost: EmbedEnvelopeSource {
    /// Resolved envelopes keyed by `primary_id` (event-id hex for nevent/note,
    /// `"kind:pubkey:d"` coordinate for naddr). Latest-snapshot-wins; rebuilt
    /// from the pre-resolved embed envelope projection on each non-nil push.
    /// The kind-dispatch runs in Rust (`nmp-content`); this class is decode-only.
    private(set) var envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [:]

    /// Diagnostics — number of resolved envelopes in the current snapshot.
    var count: Int { envelopesByPrimaryID.count }

    /// Called on every snapshot tick with the pre-resolved embed envelope map.
    /// A nil value means the projection was absent, so the previous state stays
    /// intact. An explicit empty map is authoritative and clears stale embeds.
    func update(resolvedEventEmbeds: [String: EmbeddedEventEnvelope]?) {
        guard let embeds = resolvedEventEmbeds else { return }
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
        // Linear scan only on miss. Map is small (one entry per resolved embed).
        return envelopesByPrimaryID.values.first { $0.uri == uri }
    }
}
