import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the Wave B batch #3 thin-glue projection sidecars:
/// `relay_diagnostics` (`KRDG`) and `action_lifecycle` (`KALC`). These mirror
/// `TypedPublishRelayDecoderTests`: build the typed FlatBuffers buffer directly
/// via the generated builders, wrap it in a `TypedProjectionEnvelope`, and
/// assert the generated decoder (`Typed<Key>Decoder`) produces the Chirp domain
/// value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable.
/// Each "typed present" case uses values that DIFFER from any plausible JSON
/// value (and `action_lifecycle` uses a `.failed(reason:)` whose reason is
/// distinct), so a passing assertion proves the typed path won rather than
/// coincided. The "typed absent" cases assert `nil`, which is the signal the
/// read site (`KernelModel+Projections` accessor) interprets as "fall back to
/// the generic JSON `projections.<field>` path" (ADR-0037 Commitment 4).
final class TypedDiagnosticsLifecycleDecoderTests: XCTestCase {

    // ── relay_diagnostics (KRDG) ─────────────────────────────────────────────

    func testTypedRelayDiagnosticsSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayDiagnosticsDecoder.key,
            schemaId: TypedRelayDiagnosticsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayDiagnosticsDecoder.fileIdentifier,
            payload: buildRelayDiagnostics())

        let snap = try XCTUnwrap(
            TypedRelayDiagnosticsDecoder.decode(from: [envelope]),
            "well-formed KRDG sidecar must decode")

        XCTAssertEqual(snap.relays.count, 1)
        let row = snap.relays[0]
        XCTAssertEqual(row.relayUrl, "wss://typed-diag.example")
        XCTAssertEqual(row.shortUrl, "typed-diag")
        XCTAssertEqual(row.roleLabel, "Typed Content")
        XCTAssertEqual(row.roleTone, "primary")
        XCTAssertEqual(row.connectionLabel, "Typed Connected")
        XCTAssertEqual(row.connectionTone, "ok")
        XCTAssertEqual(row.authLabel, "Typed OK")
        XCTAssertEqual(row.authTone, "ok")
        XCTAssertEqual(row.totalSubCount, 7)
        XCTAssertEqual(row.activeSubCount, 5)
        XCTAssertEqual(row.eosedSubCount, 3)
        XCTAssertEqual(row.totalEventsRx, 4242)
        XCTAssertEqual(row.totalEventsDisplay, "4.2K (typed)")
        XCTAssertEqual(row.reconnectCount, 2)
        // `has_*` present → carried string; absent → nil. Proves the typed
        // path distinguishes nil-vs-empty exactly like the JSON `null`.
        XCTAssertEqual(row.bytesRxDisplay, "12 KB (typed)")
        XCTAssertNil(row.bytesTxDisplay)
        XCTAssertEqual(row.lastConnectedDisplay, "9s ago (typed)")
        XCTAssertNil(row.lastEventDisplay)
        XCTAssertNil(row.lastNotice)
        XCTAssertEqual(row.lastError, "typed boom")

        XCTAssertEqual(row.wireSubs.count, 1)
        let sub = row.wireSubs[0]
        XCTAssertEqual(sub.wireId, "typed-wire-1")
        XCTAssertEqual(sub.shortWireId, "tw1…")
        XCTAssertEqual(sub.relayUrl, "wss://typed-diag.example")
        XCTAssertEqual(sub.filterSummary, "typed filter")
        XCTAssertEqual(sub.stateLabel, "Typed Open")
        XCTAssertEqual(sub.stateTone, "ok")
        XCTAssertEqual(sub.consumerCountLabel, "1 consumer (typed)")
        XCTAssertEqual(sub.eventsRxDisplay, "34 (typed)")
        XCTAssertTrue(sub.eoseObserved)
        XCTAssertEqual(sub.openedDisplay, "1m ago (typed)")
        XCTAssertNil(sub.lastEventDisplay)
        XCTAssertNil(sub.eoseDisplay)
        XCTAssertNil(sub.closeReason)

        XCTAssertEqual(snap.interests.count, 1)
        let interest = snap.interests[0]
        XCTAssertEqual(interest.key, "typed-interest")
        XCTAssertEqual(interest.state, "Typed Active")
        XCTAssertEqual(interest.stateTone, "ok")
        XCTAssertEqual(interest.refcount, 3)
        XCTAssertEqual(interest.cacheCoverage, "typed 80%")
        XCTAssertEqual(interest.relayUrls, ["wss://typed-a", "wss://typed-b"])
    }

    func testAbsentRelayDiagnosticsSidecarFallsBack() {
        XCTAssertNil(TypedRelayDiagnosticsDecoder.decode(from: []))
    }

    func testWrongSchemaRelayDiagnosticsFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayDiagnosticsDecoder.key,
            schemaId: "not.relay_diagnostics",
            schemaVersion: 1,
            fileIdentifier: TypedRelayDiagnosticsDecoder.fileIdentifier,
            payload: buildRelayDiagnostics())
        XCTAssertNil(TypedRelayDiagnosticsDecoder.decode(from: [envelope]))
    }

    func testGarbledRelayDiagnosticsBytesFallBack() {
        var garbled = buildRelayDiagnostics()
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedRelayDiagnosticsDecoder.decode(bytes: garbled))
    }

    func testEmptyRelayDiagnosticsPayloadFallsBack() {
        XCTAssertNil(TypedRelayDiagnosticsDecoder.decode(bytes: Data()))
    }

    /// A fresh kernel pushes an empty diagnostics buffer (no relays/interests);
    /// the typed path must decode it to the empty snapshot, NOT nil (the buffer
    /// is well-formed — falling back would be wrong here).
    func testEmptyRelayDiagnosticsSnapshotDecodesToEmpty() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayDiagnosticsDecoder.key,
            schemaId: TypedRelayDiagnosticsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayDiagnosticsDecoder.fileIdentifier,
            payload: buildEmptyRelayDiagnostics())
        let snap = try XCTUnwrap(TypedRelayDiagnosticsDecoder.decode(from: [envelope]))
        XCTAssertTrue(snap.relays.isEmpty)
        XCTAssertTrue(snap.interests.isEmpty)
    }

    // ── action_lifecycle (KALC) ──────────────────────────────────────────────

    func testTypedActionLifecycleSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedActionLifecycleDecoder.key,
            schemaId: TypedActionLifecycleDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActionLifecycleDecoder.fileIdentifier,
            payload: buildActionLifecycle())

        let snap = try XCTUnwrap(
            TypedActionLifecycleDecoder.decode(from: [envelope]),
            "well-formed KALC sidecar must decode")

        XCTAssertEqual(snap.inFlight.count, 2)
        XCTAssertEqual(snap.inFlight[0].correlationId, "typed-inflight-1")
        XCTAssertEqual(snap.inFlight[0].stage, .publishing)
        XCTAssertEqual(snap.inFlight[1].correlationId, "typed-inflight-2")
        XCTAssertEqual(snap.inFlight[1].stage, .awaitingCapability)

        XCTAssertEqual(snap.recentTerminal.count, 2)
        XCTAssertEqual(snap.recentTerminal[0].correlationId, "typed-terminal-ok")
        XCTAssertEqual(snap.recentTerminal[0].stage, .accepted)
        // `.failed(reason:)` reconstruction — the reason is distinct from any
        // plausible JSON value, so a pass proves the typed enum won.
        XCTAssertEqual(snap.recentTerminal[1].correlationId, "typed-terminal-fail")
        XCTAssertEqual(snap.recentTerminal[1].stage, .failed(reason: "TYPED relay rejected the event"))
    }

    /// An unrecognised wire stage must collapse to `.unknown(raw:)` (D1
    /// forward-compat), mirroring the JSON `init(from:)` default branch.
    func testTypedActionLifecycleUnknownStageDegrades() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedActionLifecycleDecoder.key,
            schemaId: TypedActionLifecycleDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActionLifecycleDecoder.fileIdentifier,
            payload: buildActionLifecycleUnknownStage())
        let snap = try XCTUnwrap(TypedActionLifecycleDecoder.decode(from: [envelope]))
        XCTAssertEqual(snap.inFlight.count, 1)
        XCTAssertEqual(snap.inFlight[0].stage, .unknown(raw: "future_stage_xyz"))
    }

    func testAbsentActionLifecycleSidecarFallsBack() {
        XCTAssertNil(TypedActionLifecycleDecoder.decode(from: []))
    }

    func testWrongSchemaActionLifecycleFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedActionLifecycleDecoder.key,
            schemaId: "not.action_lifecycle",
            schemaVersion: 1,
            fileIdentifier: TypedActionLifecycleDecoder.fileIdentifier,
            payload: buildActionLifecycle())
        XCTAssertNil(TypedActionLifecycleDecoder.decode(from: [envelope]))
    }

    func testGarbledActionLifecycleBytesFallBack() {
        var garbled = buildActionLifecycle()
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedActionLifecycleDecoder.decode(bytes: garbled))
    }

    func testEmptyActionLifecyclePayloadFallsBack() {
        XCTAssertNil(TypedActionLifecycleDecoder.decode(bytes: Data()))
    }

    // ── Builders ─────────────────────────────────────────────────────────────

    /// Build a KRDG buffer with one fully-populated relay row (one `has_*`
    /// present, one absent — to prove the nil-vs-empty distinction), one nested
    /// wire-sub, and one logical-interest row.
    private func buildRelayDiagnostics() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 2048)

        // Nested wire-sub: `events_rx_display` present; `last_event` / `eose` /
        // `close_reason` absent.
        let subWireId = fbb.create(string: "typed-wire-1")
        let subShort = fbb.create(string: "tw1…")
        let subRelayUrl = fbb.create(string: "wss://typed-diag.example")
        let subFilter = fbb.create(string: "typed filter")
        let subStateLabel = fbb.create(string: "Typed Open")
        let subStateTone = fbb.create(string: "ok")
        let subConsumer = fbb.create(string: "1 consumer (typed)")
        let subEventsRx = fbb.create(string: "34 (typed)")
        let subOpened = fbb.create(string: "1m ago (typed)")
        let sub = nmp_kernel_RelayDiagnosticsWireSub.createRelayDiagnosticsWireSub(
            &fbb,
            wireIdOffset: subWireId,
            shortWireIdOffset: subShort,
            relayUrlOffset: subRelayUrl,
            filterSummaryOffset: subFilter,
            stateLabelOffset: subStateLabel,
            stateToneOffset: subStateTone,
            consumerCountLabelOffset: subConsumer,
            hasEventsRxDisplay: true,
            eventsRxDisplayOffset: subEventsRx,
            eoseObserved: true,
            openedDisplayOffset: subOpened,
            hasLastEventDisplay: false,
            hasEoseDisplay: false,
            hasCloseReason: false)
        let wireSubsVec = fbb.createVector(ofOffsets: [sub])

        // Relay row: `bytes_rx` / `last_connected` / `last_error` present;
        // `bytes_tx` / `last_event` / `last_notice` absent.
        let relayUrl = fbb.create(string: "wss://typed-diag.example")
        let shortUrl = fbb.create(string: "typed-diag")
        let roleLabel = fbb.create(string: "Typed Content")
        let roleTone = fbb.create(string: "primary")
        let connLabel = fbb.create(string: "Typed Connected")
        let connTone = fbb.create(string: "ok")
        let authLabel = fbb.create(string: "Typed OK")
        let authTone = fbb.create(string: "ok")
        let totalEventsDisplay = fbb.create(string: "4.2K (typed)")
        let bytesRx = fbb.create(string: "12 KB (typed)")
        let lastConnected = fbb.create(string: "9s ago (typed)")
        let lastError = fbb.create(string: "typed boom")
        let row = nmp_kernel_RelayDiagnosticsRow.createRelayDiagnosticsRow(
            &fbb,
            relayUrlOffset: relayUrl,
            shortUrlOffset: shortUrl,
            roleLabelOffset: roleLabel,
            roleToneOffset: roleTone,
            connectionLabelOffset: connLabel,
            connectionToneOffset: connTone,
            authLabelOffset: authLabel,
            authToneOffset: authTone,
            totalSubCount: 7,
            activeSubCount: 5,
            eosedSubCount: 3,
            totalEventsRx: 4242,
            totalEventsDisplayOffset: totalEventsDisplay,
            reconnectCount: 2,
            hasBytesRxDisplay: true,
            bytesRxDisplayOffset: bytesRx,
            hasBytesTxDisplay: false,
            hasLastConnectedDisplay: true,
            lastConnectedDisplayOffset: lastConnected,
            hasLastEventDisplay: false,
            hasLastNotice: false,
            hasLastError: true,
            lastErrorOffset: lastError,
            wireSubsVectorOffset: wireSubsVec)
        let relaysVec = fbb.createVector(ofOffsets: [row])

        // Interest row with a 2-element relay-url string vector.
        let iKey = fbb.create(string: "typed-interest")
        let iState = fbb.create(string: "Typed Active")
        let iTone = fbb.create(string: "ok")
        let iCoverage = fbb.create(string: "typed 80%")
        let urlA = fbb.create(string: "wss://typed-a")
        let urlB = fbb.create(string: "wss://typed-b")
        let urlsVec = fbb.createVector(ofOffsets: [urlA, urlB])
        let interest = nmp_kernel_RelayDiagnosticsInterest.createRelayDiagnosticsInterest(
            &fbb,
            keyOffset: iKey,
            stateOffset: iState,
            stateToneOffset: iTone,
            refcount: 3,
            cacheCoverageOffset: iCoverage,
            relayUrlsVectorOffset: urlsVec)
        let interestsVec = fbb.createVector(ofOffsets: [interest])

        let root = nmp_kernel_RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(
            &fbb, relaysVectorOffset: relaysVec, interestsVectorOffset: interestsVec)
        nmp_kernel_RelayDiagnosticsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildEmptyRelayDiagnostics() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 128)
        let root = nmp_kernel_RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(&fbb)
        nmp_kernel_RelayDiagnosticsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildActionLifecycle() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)

        let entries = lifecycleVec(&fbb, [
            ("typed-inflight-1", "publishing", false, ""),
            ("typed-inflight-2", "awaiting_capability", false, ""),
        ])
        let terminals = lifecycleVec(&fbb, [
            ("typed-terminal-ok", "accepted", false, ""),
            ("typed-terminal-fail", "failed", true, "TYPED relay rejected the event"),
        ])
        let root = nmp_kernel_ActionLifecycleSnapshot.createActionLifecycleSnapshot(
            &fbb, inFlightVectorOffset: entries, recentTerminalVectorOffset: terminals)
        nmp_kernel_ActionLifecycleSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildActionLifecycleUnknownStage() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let entries = lifecycleVec(&fbb, [("typed-unknown", "future_stage_xyz", false, "")])
        let root = nmp_kernel_ActionLifecycleSnapshot.createActionLifecycleSnapshot(
            &fbb, inFlightVectorOffset: entries)
        nmp_kernel_ActionLifecycleSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func lifecycleVec(
        _ fbb: inout FlatBufferBuilder,
        _ rows: [(String, String, Bool, String)]
    ) -> Offset {
        let offsets: [Offset] = rows.map { (correlationId, stage, hasReason, reason) in
            let cidOff = fbb.create(string: correlationId)
            let stageOff = fbb.create(string: stage)
            let reasonOff = hasReason ? fbb.create(string: reason) : Offset()
            return nmp_kernel_LifecycleEntry.createLifecycleEntry(
                &fbb,
                correlationIdOffset: cidOff,
                stageOffset: stageOff,
                hasReason: hasReason,
                reasonOffset: reasonOff)
        }
        return fbb.createVector(ofOffsets: offsets)
    }
}
