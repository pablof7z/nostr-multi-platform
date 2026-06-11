import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the Wave B batch #2 thin-glue projection sidecars:
/// `configured_relays` (`KCRL`), `relay_role_options` (`KRRO`),
/// `outbox_summary` (`KOXS`), `publish_outbox` (`KPBO`), and `publish_queue`
/// (`KPBQ`). These mirror `TypedAccountsDecoderTests`: build the typed
/// FlatBuffers buffer directly via the generated builders, wrap it in a
/// `TypedProjectionEnvelope`, and assert the generated decoder
/// (`Typed<Key>Decoder`) produces the Chirp domain value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable.
/// Each "typed present" case uses values that DIFFER from any plausible JSON
/// value, so a passing assertion proves the typed path won rather than
/// coincided. The "typed absent" cases assert `nil`, which is the signal the
/// read site (`KernelModel+Projections` accessor) interprets as "fall back to
/// the generic JSON `projections.<field>` path" (ADR-0037 Commitment 4).
final class TypedPublishRelayDecoderTests: XCTestCase {

    // ── configured_relays (KCRL) ─────────────────────────────────────────────

    func testTypedConfiguredRelaysSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedConfiguredRelaysDecoder.key,
            schemaId: TypedConfiguredRelaysDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedConfiguredRelaysDecoder.fileIdentifier,
            payload: buildConfiguredRelays([
                ("wss://typed-relay-1.example", "both"),
                ("wss://typed-relay-2.example", "read,indexer"),
            ]))

        let relays = try XCTUnwrap(
            TypedConfiguredRelaysDecoder.decode(from: [envelope]),
            "well-formed KCRL sidecar must decode")

        XCTAssertEqual(relays.count, 2)
        XCTAssertEqual(relays[0].url, "wss://typed-relay-1.example")
        XCTAssertEqual(relays[0].role, "both")
        XCTAssertEqual(relays[1].url, "wss://typed-relay-2.example")
        XCTAssertEqual(relays[1].role, "read,indexer")
    }

    func testAbsentConfiguredRelaysSidecarFallsBack() {
        XCTAssertNil(TypedConfiguredRelaysDecoder.decode(from: []))
    }

    func testWrongSchemaConfiguredRelaysFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedConfiguredRelaysDecoder.key,
            schemaId: "not.configured_relays",
            schemaVersion: 1,
            fileIdentifier: TypedConfiguredRelaysDecoder.fileIdentifier,
            payload: buildConfiguredRelays([]))
        XCTAssertNil(TypedConfiguredRelaysDecoder.decode(from: [envelope]))
    }

    func testGarbledConfiguredRelaysBytesFallBack() {
        var garbled = buildConfiguredRelays([("wss://x", "read")])
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedConfiguredRelaysDecoder.decode(bytes: garbled))
    }

    func testEmptyConfiguredRelaysPayloadFallsBack() {
        XCTAssertNil(TypedConfiguredRelaysDecoder.decode(bytes: Data()))
    }

    // ── relay_role_options (KRRO) ────────────────────────────────────────────

    func testTypedRelayRoleOptionsSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayRoleOptionsDecoder.key,
            schemaId: TypedRelayRoleOptionsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayRoleOptionsDecoder.fileIdentifier,
            payload: buildRelayRoleOptions([
                ("both", "Read & Write", "accent", true),
                ("indexer", "Indexer Only", "info", false),
            ]))

        let options = try XCTUnwrap(
            TypedRelayRoleOptionsDecoder.decode(from: [envelope]),
            "well-formed KRRO sidecar must decode")

        XCTAssertEqual(options.count, 2)
        XCTAssertEqual(options[0].value, "both")
        XCTAssertEqual(options[0].label, "Read & Write")
        XCTAssertEqual(options[0].tint, "accent")
        XCTAssertTrue(options[0].isDefault)
        XCTAssertEqual(options[1].value, "indexer")
        XCTAssertEqual(options[1].label, "Indexer Only")
        XCTAssertEqual(options[1].tint, "info")
        XCTAssertFalse(options[1].isDefault)
    }

    func testAbsentRelayRoleOptionsSidecarFallsBack() {
        XCTAssertNil(TypedRelayRoleOptionsDecoder.decode(from: []))
    }

    func testGarbledRelayRoleOptionsBytesFallBack() {
        var garbled = buildRelayRoleOptions([("read", "Read", "neutral", false)])
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedRelayRoleOptionsDecoder.decode(bytes: garbled))
    }

    // ── outbox_summary (KOXS) ────────────────────────────────────────────────

    func testTypedOutboxSummarySidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedOutboxSummaryDecoder.key,
            schemaId: TypedOutboxSummaryDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedOutboxSummaryDecoder.fileIdentifier,
            payload: buildOutboxSummary(
                title: "7 pending publishes",
                subtitle: "Typed subtitle distinct from JSON",
                total: 7, sending: 3, retrying: 2, queued: 1, failed: 1))

        let summary = try XCTUnwrap(
            TypedOutboxSummaryDecoder.decode(from: [envelope]),
            "well-formed KOXS sidecar must decode")

        XCTAssertEqual(summary.title, "7 pending publishes")
        XCTAssertEqual(summary.subtitle, "Typed subtitle distinct from JSON")
        XCTAssertEqual(summary.total, 7)
        XCTAssertEqual(summary.sending, 3)
        XCTAssertEqual(summary.retrying, 2)
        XCTAssertEqual(summary.queued, 1)
        XCTAssertEqual(summary.failed, 1)
    }

    func testAbsentOutboxSummarySidecarFallsBack() {
        XCTAssertNil(TypedOutboxSummaryDecoder.decode(from: []))
    }

    func testGarbledOutboxSummaryBytesFallBack() {
        var garbled = buildOutboxSummary(
            title: "x", subtitle: "y", total: 0, sending: 0, retrying: 0, queued: 0, failed: 0)
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedOutboxSummaryDecoder.decode(bytes: garbled))
    }

    // ── publish_outbox (KPBO) ────────────────────────────────────────────────

    func testTypedPublishOutboxSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedPublishOutboxDecoder.key,
            schemaId: TypedPublishOutboxDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedPublishOutboxDecoder.fileIdentifier,
            payload: buildPublishOutbox())

        let items = try XCTUnwrap(
            TypedPublishOutboxDecoder.decode(from: [envelope]),
            "well-formed KPBO sidecar must decode")

        XCTAssertEqual(items.count, 1)
        let item = items[0]
        XCTAssertEqual(item.handle, "typed-handle-1")
        XCTAssertEqual(item.eventId, "typed-event-1")
        XCTAssertEqual(item.kind, 1)
        XCTAssertEqual(item.title, "Typed Note")
        XCTAssertEqual(item.preview, "typed preview")
        XCTAssertEqual(item.createdAtDisplay, "just now (typed)")
        XCTAssertEqual(item.status, "sending")
        XCTAssertEqual(item.statusLabel, "Sending")
        XCTAssertEqual(item.systemImage, "paperplane")
        XCTAssertTrue(item.canRetry)
        XCTAssertEqual(item.targetRelays, 2)
        XCTAssertEqual(item.targetSummary, "2 relays · typed")
        XCTAssertEqual(item.relays.count, 2)

        XCTAssertEqual(item.relays[0].relayUrl, "wss://typed-r1")
        XCTAssertEqual(item.relays[0].status, "sending")
        XCTAssertEqual(item.relays[0].statusLabel, "Sending")
        XCTAssertEqual(item.relays[0].attempt, 0)
        XCTAssertEqual(item.relays[0].attemptLabel, "")
        XCTAssertEqual(item.relays[0].message, "typed msg")
        XCTAssertEqual(item.relays[0].relayReason, "typed reason")

        // Second relay leaves `relayReason` empty — the producer's
        // `skip_serializing_if` field carried as the empty string the JSON path
        // would also yield (parity).
        XCTAssertEqual(item.relays[1].relayUrl, "wss://typed-r2")
        XCTAssertEqual(item.relays[1].attempt, 3)
        XCTAssertEqual(item.relays[1].attemptLabel, "try 3")
        XCTAssertEqual(item.relays[1].relayReason, "")
    }

    func testAbsentPublishOutboxSidecarFallsBack() {
        XCTAssertNil(TypedPublishOutboxDecoder.decode(from: []))
    }

    func testGarbledPublishOutboxBytesFallBack() {
        var garbled = buildPublishOutbox()
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedPublishOutboxDecoder.decode(bytes: garbled))
    }

    // ── publish_queue (KPBQ) ─────────────────────────────────────────────────

    /// The Chirp domain `PublishQueueEntry` is a field-SUBSET of the wire — it
    /// consumes only `eventId` / `kind` / `targetRelays` / `status`. The buffer
    /// here carries the full wire row (incl. `title` / `canRetry`); the decode
    /// must yield exactly the subset, ignoring the rest (parity with JSON).
    func testTypedPublishQueueSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedPublishQueueDecoder.key,
            schemaId: TypedPublishQueueDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedPublishQueueDecoder.fileIdentifier,
            payload: buildPublishQueue([
                ("typed-q-event-1", 30023, 4, "ok"),
                ("typed-q-event-2", 1, 9, "failed"),
            ]))

        let entries = try XCTUnwrap(
            TypedPublishQueueDecoder.decode(from: [envelope]),
            "well-formed KPBQ sidecar must decode")

        XCTAssertEqual(entries.count, 2)
        XCTAssertEqual(entries[0].eventId, "typed-q-event-1")
        XCTAssertEqual(entries[0].kind, 30023)
        XCTAssertEqual(entries[0].targetRelays, 4)
        XCTAssertEqual(entries[0].status, "ok")
        XCTAssertEqual(entries[1].eventId, "typed-q-event-2")
        XCTAssertEqual(entries[1].kind, 1)
        XCTAssertEqual(entries[1].targetRelays, 9)
        XCTAssertEqual(entries[1].status, "failed")
    }

    func testAbsentPublishQueueSidecarFallsBack() {
        XCTAssertNil(TypedPublishQueueDecoder.decode(from: []))
    }

    func testGarbledPublishQueueBytesFallBack() {
        var garbled = buildPublishQueue([("x", 1, 1, "ok")])
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedPublishQueueDecoder.decode(bytes: garbled))
    }

    // ── Builders ─────────────────────────────────────────────────────────────

    private func buildConfiguredRelays(_ rows: [(String, String)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rowOffsets: [Offset] = rows.map { (url, role) in
            let urlOff = fbb.create(string: url)
            let roleOff = fbb.create(string: role)
            return nmp_kernel_ConfiguredRelay.createConfiguredRelay(
                &fbb, urlOffset: urlOff, roleOffset: roleOff)
        }
        let vec = fbb.createVector(ofOffsets: rowOffsets)
        let root = nmp_kernel_ConfiguredRelaysSnapshot.createConfiguredRelaysSnapshot(
            &fbb, relaysVectorOffset: vec)
        nmp_kernel_ConfiguredRelaysSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildRelayRoleOptions(_ rows: [(String, String, String, Bool)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rowOffsets: [Offset] = rows.map { (value, label, tint, isDefault) in
            let valueOff = fbb.create(string: value)
            let labelOff = fbb.create(string: label)
            let tintOff = fbb.create(string: tint)
            return nmp_kernel_RelayRoleOption.createRelayRoleOption(
                &fbb, valueOffset: valueOff, labelOffset: labelOff,
                tintOffset: tintOff, isDefault: isDefault)
        }
        let vec = fbb.createVector(ofOffsets: rowOffsets)
        let root = nmp_kernel_RelayRoleOptionsSnapshot.createRelayRoleOptionsSnapshot(
            &fbb, optionsVectorOffset: vec)
        nmp_kernel_RelayRoleOptionsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildOutboxSummary(
        title: String, subtitle: String, total: UInt32, sending: UInt32,
        retrying: UInt32, queued: UInt32, failed: UInt32
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let titleOff = fbb.create(string: title)
        let subtitleOff = fbb.create(string: subtitle)
        let root = nmp_kernel_OutboxSummarySnapshot.createOutboxSummarySnapshot(
            &fbb, titleOffset: titleOff, subtitleOffset: subtitleOff,
            total: total, sending: sending, retrying: retrying,
            queued: queued, failed: failed)
        nmp_kernel_OutboxSummarySnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildPublishOutbox() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 1024)

        let r1Url = fbb.create(string: "wss://typed-r1")
        let r1Status = fbb.create(string: "sending")
        let r1Label = fbb.create(string: "Sending")
        let r1Msg = fbb.create(string: "typed msg")
        let r1Reason = fbb.create(string: "typed reason")
        let r1 = nmp_kernel_PublishOutboxRelay.createPublishOutboxRelay(
            &fbb, relayUrlOffset: r1Url, statusOffset: r1Status,
            statusLabelOffset: r1Label, attempt: 0, attemptLabelOffset: Offset(),
            messageOffset: r1Msg, relayReasonOffset: r1Reason)

        let r2Url = fbb.create(string: "wss://typed-r2")
        let r2Status = fbb.create(string: "retrying")
        let r2Label = fbb.create(string: "Retrying")
        let r2AttemptLabel = fbb.create(string: "try 3")
        let r2Msg = fbb.create(string: "")
        // relayReason intentionally omitted (Offset()) → decodes to "".
        let r2 = nmp_kernel_PublishOutboxRelay.createPublishOutboxRelay(
            &fbb, relayUrlOffset: r2Url, statusOffset: r2Status,
            statusLabelOffset: r2Label, attempt: 3, attemptLabelOffset: r2AttemptLabel,
            messageOffset: r2Msg, relayReasonOffset: Offset())

        let relaysVec = fbb.createVector(ofOffsets: [r1, r2])

        let handle = fbb.create(string: "typed-handle-1")
        let eventId = fbb.create(string: "typed-event-1")
        let title = fbb.create(string: "Typed Note")
        let preview = fbb.create(string: "typed preview")
        let created = fbb.create(string: "just now (typed)")
        let status = fbb.create(string: "sending")
        let statusLabel = fbb.create(string: "Sending")
        let systemImage = fbb.create(string: "paperplane")
        let targetSummary = fbb.create(string: "2 relays · typed")
        let item = nmp_kernel_PublishOutboxItem.createPublishOutboxItem(
            &fbb, handleOffset: handle, eventIdOffset: eventId, kind: 1,
            titleOffset: title, previewOffset: preview,
            createdAtDisplayOffset: created, statusOffset: status,
            statusLabelOffset: statusLabel, systemImageOffset: systemImage,
            canRetry: true, targetRelays: 2, targetSummaryOffset: targetSummary,
            relaysVectorOffset: relaysVec)

        let itemsVec = fbb.createVector(ofOffsets: [item])
        let root = nmp_kernel_PublishOutboxSnapshot.createPublishOutboxSnapshot(
            &fbb, itemsVectorOffset: itemsVec)
        nmp_kernel_PublishOutboxSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    /// Build a KPBQ buffer. `title` / `canRetry` / `relayOutcomes` are populated
    /// on the wire (proving the field-subset glue ignores them deterministically).
    private func buildPublishQueue(_ rows: [(String, UInt32, UInt32, String)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rowOffsets: [Offset] = rows.map { (eventId, kind, targetRelays, status) in
            let eventIdOff = fbb.create(string: eventId)
            let titleOff = fbb.create(string: "WIRE-ONLY title (ignored by glue)")
            let statusOff = fbb.create(string: status)
            return nmp_kernel_PublishQueueEntry.createPublishQueueEntry(
                &fbb, eventIdOffset: eventIdOff, kind: kind, titleOffset: titleOff,
                targetRelays: targetRelays, statusOffset: statusOff, canRetry: true,
                relayOutcomesVectorOffset: Offset())
        }
        let vec = fbb.createVector(ofOffsets: rowOffsets)
        let root = nmp_kernel_PublishQueueSnapshot.createPublishQueueSnapshot(
            &fbb, entriesVectorOffset: vec)
        nmp_kernel_PublishQueueSnapshot.finish(&fbb, end: root)
        return fbb.data
    }
}
