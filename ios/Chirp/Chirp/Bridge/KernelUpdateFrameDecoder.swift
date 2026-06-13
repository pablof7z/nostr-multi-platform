import FlatBuffers
import Foundation

enum KernelUpdateFrameDecoderError: LocalizedError {
    case emptyPayload
    case missingSnapshotPayload
    case missingPanicPayload

    var errorDescription: String? {
        switch self {
        case .emptyPayload:
            return "empty FlatBuffers update payload"
        case .missingSnapshotPayload:
            return "snapshot frame missing payload"
        case .missingPanicPayload:
            return "panic frame missing payload"
        }
    }
}

enum KernelUpdateFrame {
    /// A decoded snapshot frame. `(schemaVersion, typedProjections, flatFeeds,
    /// typedEnvelope)`. The generic `payload:Value` whole-payload tree is NO
    /// LONGER decoded — the typed `typed_projections` sidecars + the Tier-3
    /// `SnapshotFrame` envelope are the sole sources. (The producer still emits
    /// `payload` for now; PR-B removes it from the schema.)
    case snapshot(
        UInt32,
        [TypedProjectionEnvelope],
        [String: ChirpTimelineSnapshot],
        TypedSnapshotEnvelope?)
    case panic(String)
}

/// ADR-0037: a typed FlatBuffers sidecar carried alongside the generic
/// `payload` Value tree. Each envelope wraps one named projection's opaque
/// NFTS/NFCT bytes plus its schema identity. Hosts that recognise a `schemaId`
/// decode the bytes with the matching typed decoder; others ignore it and fall
/// back to the generic snapshot.
struct TypedProjectionEnvelope {
    let key: String
    let schemaId: String
    let schemaVersion: UInt32
    let fileIdentifier: String
    let payload: Data
}

enum KernelUpdateFrameDecoder {
    static func decode(_ data: Data) throws -> KernelUpdateFrame {
        guard !data.isEmpty else { throw KernelUpdateFrameDecoderError.emptyPayload }
        var buffer = ByteBuffer(data: data)
        // Buffers cross a trusted in-process FFI boundary (Rust kernel → Swift
        // shell, same process, same memory). Running getCheckedRoot here invokes
        // the FlatBuffers Verifier — an O(buffer) recursive walk — on every 4 Hz
        // snapshot frame for zero security benefit. Switch to the unchecked
        // getRoot accessor; the fileId/magic is not checked here but the
        // TypedProjectionEnvelope key+schemaId routing already selects the right
        // sub-buffer, and gross wiring errors surface at decode time as nil/empty.
        let frame: nmp_transport_UpdateFrame = getRoot(byteBuffer: &buffer)

        switch frame.kind {
        case .snapshot:
            // The generic `payload:Value` tree is intentionally NOT read here.
            // Every projection Chirp consumes now arrives through the typed
            // `typed_projections` sidecars (`envelopes`) or the Tier-3
            // `SnapshotFrame` envelope (`typedEnvelope`). The producer still
            // emits `payload` for now (PR-B removes it from the schema), so we
            // do not require it to be present.
            guard let snapshot = frame.snapshot else {
                throw KernelUpdateFrameDecoderError.missingSnapshotPayload
            }
            let envelopes = extractTypedProjections(from: snapshot)
            let flatFeeds = extractFlatFeeds(typed: envelopes)
            let typedEnvelope = extractTypedEnvelope(from: snapshot)
            return .snapshot(snapshot.schemaVersion, envelopes, flatFeeds, typedEnvelope)
        case .panic:
            guard let message = frame.panic?.msg else {
                throw KernelUpdateFrameDecoderError.missingPanicPayload
            }
            return .panic(message)
        }
    }

    /// ADR-0037: lift the typed projection sidecar into plain Swift envelopes.
    /// Projections missing a key, schema id, or payload table are skipped so a
    /// malformed entry never aborts the whole snapshot.
    private static func extractTypedProjections(
        from snapshot: nmp_transport_SnapshotFrame
    ) -> [TypedProjectionEnvelope] {
        var envelopes: [TypedProjectionEnvelope] = []
        let projections = snapshot.typedProjections
        envelopes.reserveCapacity(projections.count)
        for projection in projections {
            guard let key = projection.key,
                  let typed = projection.payload,
                  let schemaId = typed.schemaId else {
                continue
            }
            envelopes.append(TypedProjectionEnvelope(
                key: key,
                schemaId: schemaId,
                schemaVersion: typed.schemaVersion,
                fileIdentifier: typed.fileIdentifier ?? "",
                payload: Data(typed.payload)
            ))
        }
        return envelopes
    }

    /// ADR-0044 Tier-3: lift the typed `SnapshotFrame` envelope fields (read
    /// directly off the frame table, NOT the `typed_projections` sidecar) into
    /// the `TypedSnapshotEnvelope` domain value. The producer
    /// (`encode_snapshot_with_envelope`) writes ALL envelope fields as a unit
    /// whenever it carries metrics, so `metrics != nil` is the all-or-nothing
    /// presence gate: present ⇒ build the whole struct; absent (a legacy frame
    /// or the test-only no-envelope encoder) ⇒ `nil`. The bare scalars (`rev`,
    /// `running`, `last_error_toast`) have no FlatBuffers presence signal of
    /// their own — they inherit the metrics gate, which is why the whole
    /// envelope is modelled as one optional struct rather than eight. A
    /// production frame always carries metrics, so the envelope is always
    /// present in the app; a nil envelope is a non-production frame that the
    /// `apply()` staleness guard drops.
    private static func extractTypedEnvelope(
        from snapshot: nmp_transport_SnapshotFrame
    ) -> TypedSnapshotEnvelope? {
        guard let metrics = snapshot.metrics else { return nil }
        return TypedProjectionGlue.snapshotEnvelope(
            rev: snapshot.rev,
            running: snapshot.running,
            metrics: metrics,
            relayStatuses: snapshot.relayStatuses,
            logicalInterests: snapshot.logicalInterests,
            wireSubscriptions: snapshot.wireSubscriptions,
            logs: snapshot.logs,
            lastErrorToast: snapshot.lastErrorToast
        )
    }

    /// Dynamic per-view feed key prefixes the producer registers a typed `NOFS`
    /// op-feed sidecar under (`nmp.feed.author.<pk>` / `nmp.feed.thread.<id>`),
    /// the SAME shape as `nmp.feed.home`. `nmp.feed.home` itself is matched by
    /// exact key elsewhere (`TypedHomeFeedDecoder`), so it is NOT a prefix here
    /// and never collides.
    private static let flatFeedKeyPrefixes = ["nmp.feed.author.", "nmp.feed.thread."]

    /// Resolve the per-view author/thread feeds from the typed `NOFS` op-feed
    /// sidecars ONLY. Each typed envelope whose key carries an author/thread
    /// prefix AND whose `schemaId` is the op-feed descriptor is decoded through
    /// `TypedHomeFeedDecoder` (the dynamic feeds are byte-identical in shape to
    /// `nmp.feed.home`). The generic JSON `payload` projection is no longer
    /// read; #1062 made the producer emit a typed sidecar for every dynamic
    /// feed key, so the typed path is authoritative.
    private static func extractFlatFeeds(
        typed envelopes: [TypedProjectionEnvelope]
    ) -> [String: ChirpTimelineSnapshot] {
        overlayTypedFlatFeeds(json: [:], typed: envelopes)
    }

    /// Pure merge step (no FlatBuffers frame plumbing) so it is unit-testable
    /// with hand-built envelopes. Overlays typed-decoded author/thread feeds
    /// onto the JSON-derived dictionary; non-matching or undecodable envelopes
    /// leave the JSON entry in place.
    static func overlayTypedFlatFeeds(
        json: [String: ChirpTimelineSnapshot],
        typed envelopes: [TypedProjectionEnvelope]
    ) -> [String: ChirpTimelineSnapshot] {
        var feeds = json
        for envelope in envelopes {
            guard flatFeedKeyPrefixes.contains(where: { envelope.key.hasPrefix($0) }),
                  envelope.schemaId == TypedHomeFeedDecoder.schemaId,
                  let typedFeed = TypedHomeFeedDecoder.decode(bytes: envelope.payload) else {
                continue
            }
            feeds[envelope.key] = typedFeed
        }
        return feeds
    }

}
