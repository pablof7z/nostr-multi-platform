import Foundation
import Observation
import SwiftUI

/// Raw wire shape for one decoded `refs.event` row payload (KCEV). Mirrors the
/// kernel's event-row DTO fields; used by the keyed `refs.event` cache, not by a
/// host-visible `claimed_events` snapshot projection.
struct ClaimedEventDto: Decodable, Equatable {
    let id: String
    let authorPubkey: String
    let kind: Int
    let createdAt: Int
    let content: String
    let tags: [[String]]
    let signedEventJson: String?
}

/// Holds the pre-resolved embed envelope map the kernel pushes on every frame.
///
/// Issue #1283 Phase 1: the kind-dispatch + tag/JSON parsing that used to live in
/// this file (`resolve()` / `parseProfileMetadata` / `extractTopLevelMedia`) is
/// DELETED. The Rust resolver (`nmp_content::resolve_embed_projection`, invoked by
/// the `nmp-ffi` `refs.event.envelopes` sidecar producer) does that work on the
/// kernel side and ships the result as a typed `NEMB` FlatBuffer. Chirp now
/// decodes that sidecar (`TypedRefEventEnvelopesDecoder` →
/// `TypedProjectionGlue.refEventEnvelopes`) into `[String: EmbeddedEventEnvelope]`
/// and this host just stores it — ZERO embed-resolution logic in Swift
/// (D0 thin-shell; closes the EmbedHost D0 violation). This is also what fixes
/// the #1299 inverted `display_name` precedence: the kernel's NIP-01/24-correct
/// resolution is authoritative; Swift no longer re-parses kind:0 metadata.
/// Conforms to `EmbedEnvelopeSource` (owned by `content-kind-registry`'s
/// `EmbedHostEnvironment.swift`) so `NostrContentView` reads resolved envelopes
/// through the registry seam. The `embedEnvelopeSource` / `embedClaimSink` /
/// `nostrKindRegistry` environment keys live in that registry component — this
/// host no longer declares its own duplicates (F-CR-04).
@MainActor
@Observable
final class EmbedHost: EmbedEnvelopeSource {
    private(set) var envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [:]
    var count: Int { envelopesByPrimaryID.count }

    /// Called on every snapshot tick with the pre-resolved embed map decoded from
    /// the typed `refs.event.envelopes` sidecar. `nil`/empty leaves the previous
    /// map intact (stable, not flicker) — mirroring the prior behaviour.
    func update(envelopes: [String: EmbeddedEventEnvelope]?) {
        guard let envelopes, !envelopes.isEmpty else { return }
        envelopesByPrimaryID = envelopes
    }

    func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope? {
        envelopesByPrimaryID[id]
    }

    func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope? {
        if let direct = envelopesByPrimaryID[uri] { return direct }
        return envelopesByPrimaryID.values.first { $0.uri == uri }
    }
}
