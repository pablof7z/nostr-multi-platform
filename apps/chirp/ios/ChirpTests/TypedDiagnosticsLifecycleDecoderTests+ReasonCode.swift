import XCTest
import FlatBuffers
@testable import Chirp

/// Extension split for `action_lifecycle` (KALC) typed-decode tests and their
/// FlatBuffers builders. Split from `TypedDiagnosticsLifecycleDecoderTests.swift`
/// to keep each file under the 500-LOC hard cap (AGENTS.md §file-size). The
/// `relay_diagnostics` (KRDG) tests and their builders remain in the main file.
extension TypedDiagnosticsLifecycleDecoderTests {

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
        // #1735: a prose-only (un-coded) failure decodes with nil reasonCode/Subject.
        XCTAssertEqual(
            snap.recentTerminal[1].stage,
            .failed(reason: "TYPED relay rejected the event", reasonCode: nil, reasonSubject: nil))
    }

    /// #1735: a curated failure carries a `reason_code` (+ optional subject) on
    /// the wire; the typed decoder lifts both, and `localizedReason` resolves to
    /// the localized copy. An un-coded failure (above) keeps `reasonCode == nil`
    /// and `localizedReason` falls back to the prose `reason`.
    func testTypedActionLifecycleCuratedReasonCodeLifts() throws {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let cidOff = fbb.create(string: "typed-coded")
        let stageOff = fbb.create(string: "failed")
        let reasonOff = fbb.create(string: "no active account")
        let codeOff = fbb.create(string: "lifecycle_no_active_account")
        let entry = nmp_kernel_LifecycleEntry.createLifecycleEntry(
            &fbb,
            correlationIdOffset: cidOff,
            stageOffset: stageOff,
            hasReason: true,
            reasonOffset: reasonOff,
            hasReasonCode: true,
            reasonCodeOffset: codeOff)
        let recent = fbb.createVector(ofOffsets: [entry])
        let root = nmp_kernel_ActionLifecycleSnapshot.createActionLifecycleSnapshot(
            &fbb, recentTerminalVectorOffset: recent)
        nmp_kernel_ActionLifecycleSnapshot.finish(&fbb, end: root)

        let envelope = TypedProjectionEnvelope(
            key: TypedActionLifecycleDecoder.key,
            schemaId: TypedActionLifecycleDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActionLifecycleDecoder.fileIdentifier,
            payload: fbb.data)
        let snap = try XCTUnwrap(TypedActionLifecycleDecoder.decode(from: [envelope]))
        XCTAssertEqual(snap.recentTerminal.count, 1)
        XCTAssertEqual(
            snap.recentTerminal[0].stage,
            .failed(
                reason: "no active account",
                reasonCode: "lifecycle_no_active_account",
                reasonSubject: nil))
        // The shell resolves the curated code to localized copy (not the prose).
        XCTAssertEqual(
            snap.recentTerminal[0].stage.localizedReason,
            UiLifecycleReasonProse.localized(code: "lifecycle_no_active_account", subject: nil))
        XCTAssertNotEqual(snap.recentTerminal[0].stage.localizedReason, "no active account")
    }

    /// S7/#1754: a `"cancelled"` wire stage decodes to the DISTINCT
    /// `.cancelled` terminal — never `.failed`, never `.unknown`. The host
    /// renders a user-initiated cancellation without an error treatment.
    func testTypedActionLifecycleCancelledStageDecodes() throws {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let entries = lifecycleVec(&fbb, [("typed-cancelled", "cancelled", false, "")])
        let root = nmp_kernel_ActionLifecycleSnapshot.createActionLifecycleSnapshot(
            &fbb, recentTerminalVectorOffset: entries)
        nmp_kernel_ActionLifecycleSnapshot.finish(&fbb, end: root)

        let envelope = TypedProjectionEnvelope(
            key: TypedActionLifecycleDecoder.key,
            schemaId: TypedActionLifecycleDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActionLifecycleDecoder.fileIdentifier,
            payload: fbb.data)
        let snap = try XCTUnwrap(TypedActionLifecycleDecoder.decode(from: [envelope]))
        XCTAssertEqual(snap.recentTerminal.count, 1)
        XCTAssertEqual(snap.recentTerminal[0].correlationId, "typed-cancelled")
        XCTAssertEqual(snap.recentTerminal[0].stage, .cancelled)
        XCTAssertTrue(snap.recentTerminal[0].stage.isTerminal)
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

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyActionLifecyclePayloadFallsBack() {
        XCTAssertNil(TypedActionLifecycleDecoder.decode(bytes: Data()))
    }

    // ── Builders (action_lifecycle) ──────────────────────────────────────────

    func buildActionLifecycle() -> Data {
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

    func buildActionLifecycleUnknownStage() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let entries = lifecycleVec(&fbb, [("typed-unknown", "future_stage_xyz", false, "")])
        let root = nmp_kernel_ActionLifecycleSnapshot.createActionLifecycleSnapshot(
            &fbb, inFlightVectorOffset: entries)
        nmp_kernel_ActionLifecycleSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    func lifecycleVec(
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
