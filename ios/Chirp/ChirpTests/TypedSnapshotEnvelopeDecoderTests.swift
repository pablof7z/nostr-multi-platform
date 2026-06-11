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
/// whenever metrics is present). These tests pin that gate:
///
/// * `testTypedEnvelopePresentSurfacesDistinctValues` — the frame carries BOTH a
///   JSON `payload` (one set of values) AND the typed envelope (a DISTINCT set).
///   The decoder must surface the TYPED values, proving the typed path won
///   rather than coincided. Asserts FULL-struct `KernelMetrics` Equatable so a
///   silent field-swap in the glue cannot pass.
/// * `testTypedEnvelopeAbsentFallsBack` — a frame with NO metrics envelope
///   (the test-only no-envelope encode shape) yields `typedEnvelope == nil`, the
///   signal the `KernelModel+Projections` accessors read as "fall back to the
///   generic JSON `payload` top-level scalars" (ADR-0037 Commitment 4).
final class TypedSnapshotEnvelopeDecoderTests: XCTestCase {

    func testTypedEnvelopePresentSurfacesDistinctValues() throws {
        // JSON payload values (the fallback path) are deliberately DIFFERENT from
        // the typed envelope values built below, so a passing assertion proves
        // the typed envelope is what surfaced.
        let data = frameWithTypedEnvelope(
            jsonRev: 1,
            jsonRunning: false,
            typedRev: 9_001,
            typedRunning: true)

        guard case let .snapshot(_, update, _, _, typedEnvelope) =
            try KernelUpdateFrameDecoder.decode(data) else {
            return XCTFail("expected snapshot frame")
        }

        // The JSON payload decoded with the fallback values …
        XCTAssertEqual(update.rev, 1)
        XCTAssertFalse(update.running)

        // … but the typed envelope carries the DISTINCT typed values.
        let env = try XCTUnwrap(typedEnvelope, "metrics present ⇒ typed envelope built")
        XCTAssertEqual(env.rev, 9_001)
        XCTAssertTrue(env.running)

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

    func testTypedEnvelopeAbsentFallsBack() throws {
        // A frame whose SnapshotFrame omits the metrics envelope entirely (the
        // test-only `encode_snapshot_with_typed` shape) → typedEnvelope nil.
        let data = frameWithoutTypedEnvelope(jsonRev: 42, jsonRunning: true)

        guard case let .snapshot(_, update, _, _, typedEnvelope) =
            try KernelUpdateFrameDecoder.decode(data) else {
            return XCTFail("expected snapshot frame")
        }
        XCTAssertNil(typedEnvelope, "no metrics envelope ⇒ JSON fallback (nil)")
        // The JSON payload still decodes — the fallback source stays intact.
        XCTAssertEqual(update.rev, 42)
        XCTAssertTrue(update.running)
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
        dispatchDropsTotal: 41,
        duplicateEvents: 4,
        emitHzConfigured: 18,
        eoseRx: 26,
        estimatedStoreBytes: 20,
        eventsPerSecondConfigured: 17,
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

    /// Build a full `UpdateFrame` carrying a JSON `payload` (fallback values) AND
    /// the typed Tier-3 envelope (distinct values, gated on the present metrics).
    private func frameWithTypedEnvelope(
        jsonRev: Int64,
        jsonRunning: Bool,
        typedRev: UInt64,
        typedRunning: Bool
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 2048)
        let payload = jsonPayload(&fbb, rev: jsonRev, running: jsonRunning)

        let metrics = distinctMetrics(&fbb)
        let relays = fbb.createVector(ofOffsets: [distinctRelayStatus(&fbb)])
        let interests = fbb.createVector(ofOffsets: [distinctLogicalInterest(&fbb)])
        let wires = fbb.createVector(ofOffsets: [distinctWireSubscription(&fbb)])
        let logs = fbb.createVector(
            ofOffsets: ["typed log a", "typed log b"].map { fbb.create(string: $0) })

        let snapshot = nmp_transport_SnapshotFrame.createSnapshotFrame(
            &fbb,
            schemaVersion: 1,
            payloadOffset: payload,
            rev: typedRev,
            running: typedRunning,
            metricsOffset: metrics,
            relayStatusesVectorOffset: relays,
            logicalInterestsVectorOffset: interests,
            wireSubscriptionsVectorOffset: wires,
            logsVectorOffset: logs)
        let frame = nmp_transport_UpdateFrame.createUpdateFrame(
            &fbb, kind: .snapshot, snapshotOffset: snapshot)
        nmp_transport_UpdateFrame.finish(&fbb, end: frame)
        return fbb.data
    }

    /// Build an `UpdateFrame` with the JSON `payload` only — NO metrics envelope
    /// (mirrors the test-only `encode_snapshot_with_typed` wire shape).
    private func frameWithoutTypedEnvelope(jsonRev: Int64, jsonRunning: Bool) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 1024)
        let payload = jsonPayload(&fbb, rev: jsonRev, running: jsonRunning)
        let snapshot = nmp_transport_SnapshotFrame.createSnapshotFrame(
            &fbb, schemaVersion: 1, payloadOffset: payload)
        let frame = nmp_transport_UpdateFrame.createUpdateFrame(
            &fbb, kind: .snapshot, snapshotOffset: snapshot)
        nmp_transport_UpdateFrame.finish(&fbb, end: frame)
        return fbb.data
    }

    /// The generic `payload` Value tree — the minimal key set `KernelUpdate`
    /// requires (`rev`, `schema_version`, `running`, `metrics`, `relay_statuses`,
    /// `projections`). Its metrics are all-zero, distinct from the typed metrics.
    private func jsonPayload(
        _ fbb: inout FlatBufferBuilder,
        rev: Int64,
        running: Bool
    ) -> Offset {
        valueMap(&fbb, [
            ("rev", valueInt(&fbb, rev)),
            ("schema_version", valueInt(&fbb, 1)),
            ("running", valueBool(&fbb, running)),
            ("metrics", zeroMetricsValue(&fbb)),
            ("relay_statuses", valueList(&fbb, [])),
            ("projections", valueMap(&fbb, [])),
        ])
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
            eventsPerSecondConfigured: 17,
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
            dispatchDropsTotal: 41,
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

    /// All-zero metrics for the JSON fallback payload — every required metric key
    /// present so `KernelMetrics` decodes, but distinct from the typed metrics.
    private func zeroMetricsValue(_ fbb: inout FlatBufferBuilder) -> Offset {
        let keys = """
        actor_queue_depth bytes_rx bytes_tx claim_drops_total closed_rx \
        contacts_authors delete_events diagnostic_firehose_events \
        dispatch_drops_total duplicate_events emit_hz_configured eose_rx \
        estimated_store_bytes events_per_second_configured events_rx \
        events_since_last_update frames_rx generated_events inserted_count \
        make_update_us max_event_to_emit_ms max_events_per_update note_events \
        notices_rx open_views payload_bytes profile_events removed_count \
        serialize_us store_to_payload_ratio stored_events timeline_authors \
        tombstones update_frame_degradations_total update_sequence updated_count \
        visible_items visible_placeholder_avatar_items visible_profiled_items
        """.split(whereSeparator: { $0 == " " || $0 == "\n" }).map(String.init)
        return valueMap(&fbb, keys.map { ($0, valueInt(&fbb, 0)) })
    }

    // MARK: - generic Value builders (mirror OpFeedDecoderTests)

    private func valueString(_ fbb: inout FlatBufferBuilder, _ value: String) -> Offset {
        nmp_transport_Value.createValue(
            &fbb, kind: .string, stringValueOffset: fbb.create(string: value))
    }

    private func valueInt(_ fbb: inout FlatBufferBuilder, _ value: Int64) -> Offset {
        nmp_transport_Value.createValue(&fbb, kind: .int, intValue: value)
    }

    private func valueBool(_ fbb: inout FlatBufferBuilder, _ value: Bool) -> Offset {
        nmp_transport_Value.createValue(&fbb, kind: .bool, boolValue: value)
    }

    private func valueList(_ fbb: inout FlatBufferBuilder, _ values: [Offset]) -> Offset {
        nmp_transport_Value.createValue(
            &fbb, kind: .list, listVectorOffset: fbb.createVector(ofOffsets: values))
    }

    private func valueMap(_ fbb: inout FlatBufferBuilder, _ entries: [(String, Offset)]) -> Offset {
        let pairs = entries.map { key, value in
            nmp_transport_Pair.createPair(&fbb, keyOffset: fbb.create(string: key), valueOffset: value)
        }
        return nmp_transport_Value.createValue(
            &fbb, kind: .map, mapVectorOffset: fbb.createVector(ofOffsets: pairs))
    }
}
