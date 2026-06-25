import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the ADR-0044 Tier-3 `SnapshotFrame` envelope — the
/// seven first-class fields read DIRECTLY off the `SnapshotFrame` table
/// (`rev`, `running`, `metrics`, `relayStatuses`, `logicalInterests`,
/// `wireSubscriptions`, `logs`), NOT a `typed_projections` sidecar.
///
/// Unlike the per-key sidecars, the envelope scalars (`rev`, `running`) have no
/// FlatBuffers presence signal of their own, so the whole envelope is gated on
/// `SnapshotFrame.metrics != nil` (the producer
/// `encode_snapshot_with_envelope` writes ALL envelope fields as a unit
/// whenever metrics is present). These tests pin that gate and the glue:
///
/// * `testTypedEnvelopePresentSurfacesDistinctValues` — the frame carries the
///   typed envelope; the decoder maps every field through the glue. Asserts
///   FULL-struct `KernelMetrics` Equatable so a silent field-swap in the
///   ~46-field mapping cannot pass, plus the vector mappings and the re-homed
///   `last_error_toast`.
/// * `testTypedEnvelopeAbsentYieldsNil` — a frame with NO metrics envelope (the
///   test-only no-envelope encode shape) yields `typedEnvelope == nil`. A
///   production frame always carries metrics, so a nil envelope is a
///   non-production frame and `apply()` drops it (staleness guard).
final class TypedSnapshotEnvelopeDecoderTests: XCTestCase {

    func testTypedEnvelopePresentSurfacesDistinctValues() throws {
        let data = frameWithTypedEnvelope(
            typedRev: 9_001,
            typedRunning: true)

        guard case let .snapshot(_, _, _, _, _, typedEnvelope) =
            try KernelUpdateFrameDecoder.decode(data) else {
            return XCTFail("expected snapshot frame")
        }

        // The typed envelope carries the typed values, mapped field-for-field.
        let env = try XCTUnwrap(typedEnvelope, "metrics present ⇒ typed envelope built")
        XCTAssertEqual(env.rev, 9_001)
        XCTAssertTrue(env.running)
        // `last_error_toast` is re-homed onto the Tier-3 envelope (this PR).
        XCTAssertEqual(env.lastErrorToast, "typed toast")

        // Full-struct metrics equality — a glue field-swap cannot slip through.
        XCTAssertEqual(env.metrics, Self.expectedTypedMetrics)

        // Relay-status vector mapped field-for-field (the one row built below).
        XCTAssertEqual(env.relayStatuses.count, 1)
        let relay = env.relayStatuses[0]
        XCTAssertEqual(relay.relayUrl, "wss://typed.relay/x")
        XCTAssertEqual(relay.role, "read")
        XCTAssertEqual(relay.connection, "connected")
        XCTAssertEqual(relay.activeWireSubscriptions, 3)
        XCTAssertEqual(relay.reconnectCount, 2)
        XCTAssertEqual(relay.lastConnectedAtMs, 1_700_000_111)
        XCTAssertTrue(relay.denied)
        // `Option<String>` last_error present; the unset notice maps to nil.
        XCTAssertEqual(relay.lastError, "typed boom")
        XCTAssertNil(relay.lastNotice)

        // Logical-interest vector.
        XCTAssertEqual(env.logicalInterests.count, 1)
        let interest = env.logicalInterests[0]
        XCTAssertEqual(interest.key, "typed-interest")
        XCTAssertEqual(interest.state, "warming")
        XCTAssertEqual(interest.refcount, 4)
        XCTAssertEqual(interest.relayUrls, ["wss://typed.relay/x", "wss://typed.relay/y"])
        XCTAssertEqual(interest.cacheCoverage, "partial")
        XCTAssertEqual(interest.warmingUntilMs, 1_700_000_222)

        // Wire-subscription vector.
        XCTAssertEqual(env.wireSubscriptions.count, 1)
        let wire = env.wireSubscriptions[0]
        XCTAssertEqual(wire.wireId, "typed-wire-1")
        XCTAssertEqual(wire.relayUrl, "wss://typed.relay/x")
        XCTAssertEqual(wire.filterSummary, "kinds=[1]")
        XCTAssertEqual(wire.state, "open")
        XCTAssertEqual(wire.logicalConsumerCount, 5)
        XCTAssertEqual(wire.openedAtMs, 1_700_000_333)
        XCTAssertNil(wire.closeReason)

        // Logs vector mapped verbatim.
        XCTAssertEqual(env.logs, ["typed log a", "typed log b"])
    }

    func testTypedEnvelopeAbsentYieldsNil() throws {
        // A frame whose SnapshotFrame omits the metrics envelope entirely (the
        // test-only `encode_snapshot_with_typed` shape) → typedEnvelope nil.
        // This pins the metrics-presence gate: with no metrics, no envelope is
        // built. A production frame always carries metrics, so a nil envelope
        // is a non-production frame and `apply()` drops it (staleness guard).
        let data = frameWithoutTypedEnvelope()

        guard case let .snapshot(_, _, _, _, _, typedEnvelope) =
            try KernelUpdateFrameDecoder.decode(data) else {
            return XCTFail("expected snapshot frame")
        }
        XCTAssertNil(typedEnvelope, "no metrics envelope ⇒ nil (metrics-presence gate)")
    }

    // MARK: - Expected typed metrics (full struct)

    /// The exact `KernelMetrics` the `metricsValue(distinct:)` builder below
    /// produces. Asserting against this whole struct (not a spot field) is what
    /// makes the glue's ~46-field mapping safe against a silent swap.
    private static let expectedTypedMetrics = KernelMetrics(
        actorQueueDepth: 23,
        bytesRx: 24,
        bytesTx: 25,
        claimDropsTotal: 42,
        closedRx: 28,
        contactsAuthors: 31,
        deleteEvents: 5,
        diagnosticFirehoseEvents: 13,
        duplicateEvents: 4,
        emitHzConfigured: 18,
        eoseRx: 26,
        estimatedStoreBytes: 20,
        eventsRx: 25_000,
        eventsSinceLastUpdate: 12,
        firstEventMs: 1_700_000_001,
        framesRx: 24_000,
        generatedEvents: 1,
        insertedCount: 14,
        lastEventToEmitMs: 1_700_000_006,
        makeUpdateUs: 44,
        maxEventToEmitMs: 39,
        maxEventsPerUpdate: 40,
        noteEvents: 2,
        noticesRx: 27,
        openViews: 11,
        payloadBytes: 21,
        profileEvents: 3,
        removedCount: 16,
        serializeUs: 45,
        storeToPayloadRatio: 2.5,
        storedEvents: 6,
        targetProfileLoadedMs: 1_700_000_002,
        timelineAuthors: 32,
        timelineFirstItemMs: 1_700_000_004,
        timelineOpenedMs: 1_700_000_003,
        tombstones: 7,
        updateEmittedMs: 1_700_000_005,
        updateFrameDegradationsTotal: 46,
        updateSequence: 19,
        updatedCount: 15,
        visibleItems: 8,
        visiblePlaceholderAvatarItems: 10,
        visibleProfiledItems: 9)

    // MARK: - FlatBuffers builders

    /// Build an `UpdateFrame` carrying the typed Tier-3 envelope (gated on the
    /// present metrics). The producer still attaches a generic `payload` on the
    /// frame for now (PR-B removes it from the schema); the decoder no longer
    /// reads it, so the payload values are irrelevant — a placeholder is set so
    /// the frame shape matches production.
    private func frameWithTypedEnvelope(
        typedRev: UInt64,
        typedRunning: Bool
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 2048)

        let metrics = distinctMetrics(&fbb)
        let relays = fbb.createVector(ofOffsets: [distinctRelayStatus(&fbb)])
        let interests = fbb.createVector(ofOffsets: [distinctLogicalInterest(&fbb)])
        let wires = fbb.createVector(ofOffsets: [distinctWireSubscription(&fbb)])
        let logs = fbb.createVector(
            ofOffsets: ["typed log a", "typed log b"].map { fbb.create(string: $0) })
        let lastErrorToast = fbb.create(string: "typed toast")

        let snapshot = nmp_transport_SnapshotFrame.createSnapshotFrame(
            &fbb,
            schemaVersion: 1,
            rev: typedRev,
            running: typedRunning,
            metricsOffset: metrics,
            relayStatusesVectorOffset: relays,
            logicalInterestsVectorOffset: interests,
            wireSubscriptionsVectorOffset: wires,
            logsVectorOffset: logs,
            lastErrorToastOffset: lastErrorToast)
        let frame = nmp_transport_UpdateFrame.createUpdateFrame(
            &fbb, kind: .snapshot, snapshotOffset: snapshot)
        nmp_transport_UpdateFrame.finish(&fbb, end: frame)
        return fbb.data
    }

    /// Build an `UpdateFrame` with NO metrics envelope (mirrors the test-only
    /// `encode_snapshot_with_typed` wire shape), so `typedEnvelope` is nil.
    private func frameWithoutTypedEnvelope() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 1024)
        let snapshot = nmp_transport_SnapshotFrame.createSnapshotFrame(
            &fbb, schemaVersion: 1)
        let frame = nmp_transport_UpdateFrame.createUpdateFrame(
            &fbb, kind: .snapshot, snapshotOffset: snapshot)
        nmp_transport_UpdateFrame.finish(&fbb, end: frame)
        return fbb.data
    }

    /// The typed `Metrics` table with the distinct values pinned by
    /// `expectedTypedMetrics`.
    private func distinctMetrics(_ fbb: inout FlatBufferBuilder) -> Offset {
        nmp_transport_Metrics.createMetrics(
            &fbb,
            generatedEvents: 1,
            noteEvents: 2,
            profileEvents: 3,
            duplicateEvents: 4,
            deleteEvents: 5,
            storedEvents: 6,
            tombstones: 7,
            visibleItems: 8,
            visibleProfiledItems: 9,
            visiblePlaceholderAvatarItems: 10,
            openViews: 11,
            eventsSinceLastUpdate: 12,
            diagnosticFirehoseEvents: 13,
            insertedCount: 14,
            updatedCount: 15,
            removedCount: 16,
            emitHzConfigured: 18,
            updateSequence: 19,
            estimatedStoreBytes: 20,
            payloadBytes: 21,
            storeToPayloadRatio: 2.5,
            actorQueueDepth: 23,
            framesRx: 24_000,
            eventsRx: 25_000,
            eoseRx: 26,
            noticesRx: 27,
            closedRx: 28,
            bytesRx: 24,
            bytesTx: 25,
            contactsAuthors: 31,
            timelineAuthors: 32,
            firstEventMs: 1_700_000_001,
            targetProfileLoadedMs: 1_700_000_002,
            timelineOpenedMs: 1_700_000_003,
            timelineFirstItemMs: 1_700_000_004,
            updateEmittedMs: 1_700_000_005,
            lastEventToEmitMs: 1_700_000_006,
            maxEventToEmitMs: 39,
            maxEventsPerUpdate: 40,
            claimDropsTotal: 42,
            makeUpdateUs: 44,
            serializeUs: 45,
            updateFrameDegradationsTotal: 46)
    }

    private func distinctRelayStatus(_ fbb: inout FlatBufferBuilder) -> Offset {
        nmp_transport_RelayStatus.createRelayStatus(
            &fbb,
            roleOffset: fbb.create(string: "read"),
            relayUrlOffset: fbb.create(string: "wss://typed.relay/x"),
            connectionOffset: fbb.create(string: "connected"),
            authOffset: fbb.create(string: "none"),
            negentropyProbeOffset: fbb.create(string: "unsupported"),
            activeWireSubscriptions: 3,
            reconnectCount: 2,
            lastConnectedAtMs: 1_700_000_111,
            lastErrorOffset: fbb.create(string: "typed boom"),
            errorCategoryOffset: fbb.create(string: "network"),
            eventsRx: 50,
            bytesRx: 60,
            bytesTx: 70,
            denied: true)
    }

    private func distinctLogicalInterest(_ fbb: inout FlatBufferBuilder) -> Offset {
        let urls = fbb.createVector(
            ofOffsets: ["wss://typed.relay/x", "wss://typed.relay/y"].map {
                fbb.create(string: $0)
            })
        return nmp_transport_LogicalInterestStatus.createLogicalInterestStatus(
            &fbb,
            keyOffset: fbb.create(string: "typed-interest"),
            stateOffset: fbb.create(string: "warming"),
            refcount: 4,
            relayUrlsVectorOffset: urls,
            cacheCoverageOffset: fbb.create(string: "partial"),
            warmingUntilMs: 1_700_000_222)
    }

    private func distinctWireSubscription(_ fbb: inout FlatBufferBuilder) -> Offset {
        nmp_transport_WireSubscriptionStatus.createWireSubscriptionStatus(
            &fbb,
            wireIdOffset: fbb.create(string: "typed-wire-1"),
            relayUrlOffset: fbb.create(string: "wss://typed.relay/x"),
            filterSummaryOffset: fbb.create(string: "kinds=[1]"),
            stateOffset: fbb.create(string: "open"),
            logicalConsumerCount: 5,
            eventsRx: 80,
            openedAtMs: 1_700_000_333)
    }

}
