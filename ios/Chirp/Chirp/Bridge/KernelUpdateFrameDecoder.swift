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
    /// A decoded snapshot frame. `(schemaVersion, sessionId, snapshotEpoch,
    /// typedProjections, flatFeeds, typedEnvelope)`. The generic `payload:Value`
    /// whole-payload tree is NO LONGER decoded — the typed `typed_projections`
    /// sidecars + the Tier-3 `SnapshotFrame` envelope are the sole sources. (The
    /// producer still emits `payload` for now; PR-B removes it from the schema.)
    ///
    /// R3-S3 (ADR-0055): `sessionId` + `snapshotEpoch` are read off the SAME
    /// `frame.snapshot` table in the single decode pass and threaded out here so
    /// the `ProjectionMergeCache` D3-3 merge needs no second parse of the buffer.
    case snapshot(
        UInt32,
        UInt64,
        UInt64,
        [TypedProjectionEnvelope],
        [String: OpFeedSnapshot],
        TypedSnapshotEnvelope?)
    case panic(String)
}

/// The wire presence state of one typed projection row, mirroring the
/// `nmp_transport_ProjectionPresenceState` FlatBuffers enum (ubyte values
/// must match exactly). `Unchanged` is never on the wire — absence IS
/// Unchanged per ADR-0055 D3.
///
/// Raw values match the FlatBuffers ubyte encoding:
///   0 = Changed  (full payload row)
///   1 = Cleared  (payload-less row; host must drop the cached value)
enum WireProjectionState: UInt8 {
    case changed = 0
    case cleared = 1
}

/// ADR-0037: a typed FlatBuffers sidecar carried alongside the generic
/// `payload` Value tree. Each envelope wraps one named projection's opaque
/// NFTS/NFCT bytes plus its schema identity. Hosts that recognise a `schemaId`
/// decode the bytes with the matching typed decoder; others ignore it and fall
/// back to the generic snapshot.
///
/// R3-S3 (ADR-0055): `projectionRev` and `state` are now populated from
/// the FlatBuffers `TypedProjection` fields so the `ProjectionMergeCache`
/// can implement the D3-3 merge algorithm without re-reading the wire.
struct TypedProjectionEnvelope {
    let key: String
    let schemaId: String
    let schemaVersion: UInt32
    let fileIdentifier: String
    let payload: Data
    /// Per-projection monotonic revision counter. Advances on every semantic
    /// change; the cache-merge layer uses this to implement the D3 reorder
    /// guard (lower-or-equal rev → skip; advancing rev → overwrite).
    ///
    /// Default `1` is used by decoder-only tests that do not exercise the
    /// cache layer — a non-zero rev so the reorder guard never rejects, but
    /// small enough to leave room for cache tests to assert on specific values.
    let projectionRev: UInt64
    /// Wire presence state for this row. `.changed` means full payload;
    /// `.cleared` means the projection went empty (payload is empty Data).
    /// `Unchanged` is never on the wire — omitted rows ARE unchanged.
    ///
    /// Default `.changed` is used by decoder-only tests that construct
    /// well-formed payload envelopes and do not test cache semantics.
    let state: WireProjectionState

    init(
        key: String,
        schemaId: String,
        schemaVersion: UInt32,
        fileIdentifier: String,
        payload: Data,
        projectionRev: UInt64 = 1,
        state: WireProjectionState = .changed
    ) {
        self.key = key
        self.schemaId = schemaId
        self.schemaVersion = schemaVersion
        self.fileIdentifier = fileIdentifier
        self.payload = payload
        self.projectionRev = projectionRev
        self.state = state
    }
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
            // R3-S3 (ADR-0055): read the session/epoch scalars off the SAME
            // `snapshot` table we already decoded — no second parse needed.
            return .snapshot(
                snapshot.schemaVersion,
                snapshot.sessionId,
                snapshot.snapshotEpoch,
                envelopes,
                flatFeeds,
                typedEnvelope)
        case .panic:
            guard let message = frame.panic?.msg else {
                throw KernelUpdateFrameDecoderError.missingPanicPayload
            }
            return .panic(message)
        }
    }

    /// ADR-0037: lift the typed projection sidecar into plain Swift envelopes.
    /// Projections missing a key are skipped so a malformed entry never aborts
    /// the whole snapshot.
    ///
    /// R3-S3 (ADR-0055): `projectionRev` and `state` are now populated from the
    /// FlatBuffers `TypedProjection` fields. `Cleared` rows carry no payload
    /// table — that is correct and expected; the cache-merge layer handles them
    /// by removing the key from the cache. The envelope `payload` is `Data()`
    /// for Cleared rows.
    private static func extractTypedProjections(
        from snapshot: nmp_transport_SnapshotFrame
    ) -> [TypedProjectionEnvelope] {
        var envelopes: [TypedProjectionEnvelope] = []
        let projections = snapshot.typedProjections
        envelopes.reserveCapacity(projections.count)
        for projection in projections {
            guard let key = projection.key else { continue }
            // Map the FlatBuffers ubyte enum to our typed WireProjectionState.
            // Default to .changed when the field is absent (legacy frames or
            // Tier-1 always-Changed rows that pre-date Rung-2 stamping).
            let state: WireProjectionState = projection.state == .cleared ? .cleared : .changed
            let projectionRev = projection.projectionRev
            // Cleared rows carry no payload table — extract what is present.
            let (schemaId, schemaVersion, fileIdentifier, payload): (String, UInt32, String, Data)
            if let typed = projection.payload, let sid = typed.schemaId {
                (schemaId, schemaVersion, fileIdentifier, payload) = (
                    sid,
                    typed.schemaVersion,
                    typed.fileIdentifier ?? "",
                    Data(typed.payload)
                )
            } else if state == .cleared {
                // Cleared rows have no payload; fill identity fields with empty
                // so the envelope is well-formed for the cache-merge layer.
                (schemaId, schemaVersion, fileIdentifier, payload) = ("", 0, "", Data())
            } else {
                // Changed row without a payload table — malformed; skip.
                continue
            }
            envelopes.append(TypedProjectionEnvelope(
                key: key,
                schemaId: schemaId,
                schemaVersion: schemaVersion,
                fileIdentifier: fileIdentifier,
                payload: payload,
                projectionRev: projectionRev,
                state: state
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
            lastErrorToast: snapshot.lastErrorToast,
            lastErrorCategory: snapshot.lastErrorCategory
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
    ) -> [String: OpFeedSnapshot] {
        overlayTypedFlatFeeds(json: [:], typed: envelopes)
    }

    /// Pure merge step (no FlatBuffers frame plumbing) so it is unit-testable
    /// with hand-built envelopes. Overlays typed-decoded author/thread feeds
    /// onto the JSON-derived dictionary; non-matching or undecodable envelopes
    /// leave the JSON entry in place.
    static func overlayTypedFlatFeeds(
        json: [String: OpFeedSnapshot],
        typed envelopes: [TypedProjectionEnvelope]
    ) -> [String: OpFeedSnapshot] {
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
