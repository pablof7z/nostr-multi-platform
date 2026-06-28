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

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyConfiguredRelaysPayloadFallsBack() {
        XCTAssertNil(TypedConfiguredRelaysDecoder.decode(bytes: Data()))
    }

    // ── relay_role_options (KRRO) ────────────────────────────────────────────

    func testTypedRelayRoleOptionsSidecarDecodes() throws {
        // `label` was removed from the wire (#1678, D7). Tuples are now
        // (value, tint, isDefault); the shell computes `label` from `value`.
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayRoleOptionsDecoder.key,
            schemaId: TypedRelayRoleOptionsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayRoleOptionsDecoder.fileIdentifier,
            payload: buildRelayRoleOptions([
                ("both", "accent", true),
                ("indexer", "info", false),
            ]))

        let options = try XCTUnwrap(
            TypedRelayRoleOptionsDecoder.decode(from: [envelope]),
            "well-formed KRRO sidecar must decode")

        XCTAssertEqual(options.count, 2)
        XCTAssertEqual(options[0].value, "both")
        // `label` is a computed shell property mapping value → English string (#1678).
        XCTAssertEqual(options[0].label, "Both")
        XCTAssertEqual(options[0].tint, "accent")
        XCTAssertTrue(options[0].isDefault)
        XCTAssertEqual(options[1].value, "indexer")
        XCTAssertEqual(options[1].label, "Index")
        XCTAssertEqual(options[1].tint, "info")
        XCTAssertFalse(options[1].isDefault)
    }

    func testAbsentRelayRoleOptionsSidecarFallsBack() {
        XCTAssertNil(TypedRelayRoleOptionsDecoder.decode(from: []))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // ── outbox_summary (KOXS) ────────────────────────────────────────────────

    func testTypedOutboxSummarySidecarDecodes() throws {
        // ADR-0032 / aim.md §2 #4: `title` / `subtitle` removed from the wire.
        let envelope = TypedProjectionEnvelope(
            key: TypedOutboxSummaryDecoder.key,
            schemaId: TypedOutboxSummaryDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedOutboxSummaryDecoder.fileIdentifier,
            payload: buildOutboxSummary(
                total: 7, sending: 3, retrying: 2, queued: 1, failed: 1))

        let summary = try XCTUnwrap(
            TypedOutboxSummaryDecoder.decode(from: [envelope]),
            "well-formed KOXS sidecar must decode")

        XCTAssertEqual(summary.total, 7)
        XCTAssertEqual(summary.sending, 3)
        XCTAssertEqual(summary.retrying, 2)
        XCTAssertEqual(summary.queued, 1)
        XCTAssertEqual(summary.failed, 1)
        // Shell-computed display strings:
        XCTAssertEqual(summary.displayTitle, "7 pending publishes")
        XCTAssertTrue(summary.displaySubtitle.contains("3 currently sending"))
    }

    func testAbsentOutboxSummarySidecarFallsBack() {
        XCTAssertNil(TypedOutboxSummaryDecoder.decode(from: []))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // ── publish_outbox (KPBO) ────────────────────────────────────────────────

    func testTypedPublishOutboxSidecarDecodes() throws {
        // ADR-0032 / aim.md §2 #4: `title`, `preview`, `statusLabel`,
        // `systemImage` removed from the wire; `content` added.
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
        XCTAssertEqual(item.content, "typed content body")
        // ADR-0032 / V-115: `createdAtDisplay`/`targetSummary` removed; assert
        // raw unix-seconds and relay count instead.
        XCTAssertEqual(item.createdAt, 1_700_000_000)
        XCTAssertEqual(item.status, "sending")
        XCTAssertTrue(item.canRetry)
        XCTAssertEqual(item.targetRelays, 2)
        XCTAssertEqual(item.relays.count, 2)
        // Shell-computed display helpers:
        XCTAssertEqual(item.kindTitle, "Note")
        XCTAssertEqual(item.iconName, "text.bubble")
        XCTAssertEqual(item.statusLabel, "Sending")

        XCTAssertEqual(item.relays[0].relayUrl, "wss://typed-r1")
        XCTAssertEqual(item.relays[0].status, "sending")
        XCTAssertEqual(item.relays[0].attempt, 0)
        XCTAssertEqual(item.relays[0].message, "typed msg")
        XCTAssertEqual(item.relays[0].relayReason, "typed reason")
        // Shell-computed relay display helpers:
        XCTAssertEqual(item.relays[0].statusLabel, "Sending")
        XCTAssertEqual(item.relays[0].attemptLabel, "")

        // Second relay leaves `relayReason` empty — the producer's
        // `skip_serializing_if` field carried as the empty string the JSON path
        // would also yield (parity).
        XCTAssertEqual(item.relays[1].relayUrl, "wss://typed-r2")
        XCTAssertEqual(item.relays[1].attempt, 3)
        XCTAssertEqual(item.relays[1].relayReason, "")
        // Shell-computed:
        XCTAssertEqual(item.relays[1].attemptLabel, "try 3")
        XCTAssertEqual(item.relays[1].statusLabel, "Retrying")
    }

    func testAbsentPublishOutboxSidecarFallsBack() {
        XCTAssertNil(TypedPublishOutboxDecoder.decode(from: []))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

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

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

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

    // `label` removed from wire (#1678, D7). Tuple is now (value, tint, isDefault).
    private func buildRelayRoleOptions(_ rows: [(String, String, Bool)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rowOffsets: [Offset] = rows.map { (value, tint, isDefault) in
            let valueOff = fbb.create(string: value)
            let tintOff = fbb.create(string: tint)
            // `labelOffset` vtable slot deprecated; omit — default Offset() writes nothing.
            return nmp_kernel_RelayRoleOption.createRelayRoleOption(
                &fbb, valueOffset: valueOff,
                tintOffset: tintOff, isDefault: isDefault)
        }
        let vec = fbb.createVector(ofOffsets: rowOffsets)
        let root = nmp_kernel_RelayRoleOptionsSnapshot.createRelayRoleOptionsSnapshot(
            &fbb, optionsVectorOffset: vec)
        nmp_kernel_RelayRoleOptionsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildOutboxSummary(
        total: UInt32, sending: UInt32,
        retrying: UInt32, queued: UInt32, failed: UInt32
    ) -> Data {
        // ADR-0032 / aim.md §2 #4: `title` / `subtitle` removed from the wire.
        var fbb = FlatBufferBuilder(initialSize: 256)
        let root = nmp_kernel_OutboxSummarySnapshot.createOutboxSummarySnapshot(
            &fbb, total: total, sending: sending, retrying: retrying,
            queued: queued, failed: failed)
        nmp_kernel_OutboxSummarySnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildPublishOutbox() -> Data {
        // ADR-0032 / aim.md §2 #4: `title`, `preview`, `statusLabel`,
        // `systemImage`, relay `statusLabel`, relay `attemptLabel` removed.
        // `content` added to item.
        var fbb = FlatBufferBuilder(initialSize: 1024)

        let r1Url = fbb.create(string: "wss://typed-r1")
        let r1Status = fbb.create(string: "sending")
        let r1Msg = fbb.create(string: "typed msg")
        let r1Reason = fbb.create(string: "typed reason")
        let r1 = nmp_kernel_PublishOutboxRelay.createPublishOutboxRelay(
            &fbb, relayUrlOffset: r1Url, statusOffset: r1Status,
            attempt: 0, messageOffset: r1Msg, relayReasonOffset: r1Reason)

        let r2Url = fbb.create(string: "wss://typed-r2")
        let r2Status = fbb.create(string: "retrying")
        let r2Msg = fbb.create(string: "")
        // relayReason intentionally omitted (Offset()) → decodes to "".
        let r2 = nmp_kernel_PublishOutboxRelay.createPublishOutboxRelay(
            &fbb, relayUrlOffset: r2Url, statusOffset: r2Status,
            attempt: 3, messageOffset: r2Msg, relayReasonOffset: Offset())

        let relaysVec = fbb.createVector(ofOffsets: [r1, r2])

        let handle = fbb.create(string: "typed-handle-1")
        let eventId = fbb.create(string: "typed-event-1")
        let content = fbb.create(string: "typed content body")
        // ADR-0032 / V-115: use raw unix seconds; no createdAtDisplay/targetSummary.
        let status = fbb.create(string: "sending")
        let item = nmp_kernel_PublishOutboxItem.createPublishOutboxItem(
            &fbb, handleOffset: handle, eventIdOffset: eventId, kind: 1,
            statusOffset: status, canRetry: true, targetRelays: 2,
            relaysVectorOffset: relaysVec, createdAt: 1_700_000_000,
            contentOffset: content)

        let itemsVec = fbb.createVector(ofOffsets: [item])
        let root = nmp_kernel_PublishOutboxSnapshot.createPublishOutboxSnapshot(
            &fbb, itemsVectorOffset: itemsVec)
        nmp_kernel_PublishOutboxSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    /// Build a KPBQ buffer. `canRetry` / `relayOutcomes` are populated on the
    /// wire (proving the field-subset glue ignores them deterministically).
    private func buildPublishQueue(_ rows: [(String, UInt32, UInt32, String)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let rowOffsets: [Offset] = rows.map { (eventId, kind, targetRelays, status) in
            let eventIdOff = fbb.create(string: eventId)
            let statusOff = fbb.create(string: status)
            return nmp_kernel_PublishQueueEntry.createPublishQueueEntry(
                &fbb, eventIdOffset: eventIdOff, kind: kind,
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
