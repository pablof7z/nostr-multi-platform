import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the NIP-46 cluster sidecars: `bunker_handshake`
/// (`KBHS`) and `nip46_onboarding` (`KN46`).
///
/// These mirror `TypedDmClusterDecoderTests` / `TypedProfileClusterDecoderTests`:
/// build the typed FlatBuffers buffer directly via the generated builders, wrap
/// it in a `TypedProjectionEnvelope` carrying the producer's actual
/// `(key, schemaId)`, and assert the generated `Typed<Key>Decoder` produces the
/// Chirp domain value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable. Each
/// "typed present" case uses values that DIFFER from any plausible JSON value,
/// so a passing assertion proves the typed path won rather than coincided. The
/// "absent / wrong-schema / garbled" cases assert `nil`, the signal the read
/// site interprets as "fall back to the generic JSON `projections.<field>` path"
/// (ADR-0037 Commitment 4). For both keys `key == schemaId`, so the
/// `*IdentityIsExact` cases pin the producer contract cheaply.
///
/// `has_*` companion-bool semantics (`hasMessage` on the handshake,
/// `hasStageKind` / `hasProgressMessage` on the onboarding model) are pinned to
/// map `false → nil`, reproducing the JSON `null`-when-`None` shape regardless
/// of the empty string slot.
final class TypedNip46ClusterDecoderTests: XCTestCase {

    // MARK: - bunker_handshake (KBHS)

    func testBunkerHandshakeSidecarIdentityIsExact() {
        XCTAssertEqual(TypedBunkerHandshakeDecoder.key, "bunker_handshake")
        XCTAssertEqual(TypedBunkerHandshakeDecoder.schemaId, "bunker_handshake")
        XCTAssertEqual(TypedBunkerHandshakeDecoder.fileIdentifier, "KBHS")
    }

    func testTypedBunkerHandshakeSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedBunkerHandshakeDecoder.key,
            schemaId: TypedBunkerHandshakeDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedBunkerHandshakeDecoder.fileIdentifier,
            payload: buildBunkerHandshake(
                stage: "typed_connecting",
                message: "typed connecting to bunker",
                isIdle: false,
                isInFlight: true,
                isFailed: false,
                isTerminalSuccess: false,
                canCancel: true,
                stageLabel: "Typed: Connecting to bunker relays…"))

        let dto = try XCTUnwrap(
            TypedBunkerHandshakeDecoder.decode(from: [envelope]),
            "well-formed KBHS sidecar must decode")

        // Distinct-from-JSON values prove the typed path won.
        XCTAssertEqual(dto.stage, "typed_connecting")
        XCTAssertEqual(dto.message, "typed connecting to bunker")
        XCTAssertEqual(dto.isIdle, false)
        XCTAssertEqual(dto.isInFlight, true)
        XCTAssertEqual(dto.isFailed, false)
        XCTAssertEqual(dto.isTerminalSuccess, false)
        XCTAssertEqual(dto.canCancel, true)
        XCTAssertEqual(dto.stageLabel, "Typed: Connecting to bunker relays…")
    }

    /// `has_message == false` (no status text) → nil message, reproducing the
    /// JSON `null`-when-`None` shape.
    func testTypedBunkerHandshakeAbsentMessageMapsToNil() throws {
        let dto = try XCTUnwrap(
            TypedBunkerHandshakeDecoder.decode(
                bytes: buildBunkerHandshake(
                    stage: "ready",
                    message: nil,
                    isIdle: false,
                    isInFlight: false,
                    isFailed: false,
                    isTerminalSuccess: true,
                    canCancel: false,
                    stageLabel: "Connected")))
        XCTAssertNil(dto.message)
        XCTAssertEqual(dto.stage, "ready")
        XCTAssertEqual(dto.isTerminalSuccess, true)
    }

    func testAbsentBunkerHandshakeSidecarFallsBack() {
        XCTAssertNil(TypedBunkerHandshakeDecoder.decode(from: []))
    }

    func testWrongSchemaBunkerHandshakeFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedBunkerHandshakeDecoder.key,
            schemaId: "not.bunker_handshake",
            schemaVersion: 1,
            fileIdentifier: TypedBunkerHandshakeDecoder.fileIdentifier,
            payload: buildBunkerHandshake(
                stage: "idle", message: nil, isIdle: true, isInFlight: false,
                isFailed: false, isTerminalSuccess: false, canCancel: false,
                stageLabel: "Idle"))
        XCTAssertNil(TypedBunkerHandshakeDecoder.decode(from: [envelope]))
    }

    func testGarbledBunkerHandshakeBytesFallBack() {
        var garbled = buildBunkerHandshake(
            stage: "idle", message: nil, isIdle: true, isInFlight: false,
            isFailed: false, isTerminalSuccess: false, canCancel: false,
            stageLabel: "Idle")
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedBunkerHandshakeDecoder.decode(bytes: garbled))
    }

    // MARK: - nip46_onboarding (KN46)

    func testNip46OnboardingSidecarIdentityIsExact() {
        XCTAssertEqual(TypedNip46OnboardingDecoder.key, "nip46_onboarding")
        XCTAssertEqual(TypedNip46OnboardingDecoder.schemaId, "nip46_onboarding")
        XCTAssertEqual(TypedNip46OnboardingDecoder.fileIdentifier, "KN46")
    }

    func testTypedNip46OnboardingSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedNip46OnboardingDecoder.key,
            schemaId: TypedNip46OnboardingDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedNip46OnboardingDecoder.fileIdentifier,
            payload: buildNip46Onboarding(
                signerApps: [
                    SignerAppFixture(
                        scheme: "typedsigner://", displayLabel: "Typed Signer",
                        signerKind: "nip46"),
                    SignerAppFixture(
                        scheme: "typedprimal://", displayLabel: "Typed Primal",
                        signerKind: "nip46"),
                ],
                stageKind: "awaiting_pubkey",
                progressMessage: "typed waiting for approval",
                isInFlight: true,
                isFailed: false,
                isTerminalSuccess: false,
                canCancel: true))

        let dto = try XCTUnwrap(
            TypedNip46OnboardingDecoder.decode(from: [envelope]),
            "well-formed KN46 sidecar must decode")

        // Signer-app order preserved verbatim (Rust owns the table).
        XCTAssertEqual(dto.signerApps.map(\.scheme), ["typedsigner://", "typedprimal://"])
        XCTAssertEqual(dto.signerApps[0].displayLabel, "Typed Signer")
        XCTAssertEqual(dto.signerApps[0].signerKind, "nip46")
        // The snake_case wire token re-types to the SAME StageKind enum.
        XCTAssertEqual(dto.stageKind, .awaitingPubkey)
        XCTAssertEqual(dto.progressMessage, "typed waiting for approval")
        XCTAssertTrue(dto.isInFlight)
        XCTAssertFalse(dto.isFailed)
        XCTAssertFalse(dto.isTerminalSuccess)
        XCTAssertTrue(dto.canCancel)
    }

    /// `has_stage_kind == false` (no handshake in flight) → nil stageKind;
    /// `has_progress_message == false` → nil progressMessage. Both reproduce the
    /// JSON `null`-when-`None` shape; `signerApps` stays present.
    func testTypedNip46OnboardingAbsentOptionalsMapToNil() throws {
        let dto = try XCTUnwrap(
            TypedNip46OnboardingDecoder.decode(
                bytes: buildNip46Onboarding(
                    signerApps: [
                        SignerAppFixture(
                            scheme: "nostrsigner://", displayLabel: "Signer",
                            signerKind: "nip46"),
                    ],
                    stageKind: nil,
                    progressMessage: nil,
                    isInFlight: false,
                    isFailed: false,
                    isTerminalSuccess: false,
                    canCancel: false)))
        XCTAssertNil(dto.stageKind)
        XCTAssertNil(dto.progressMessage)
        XCTAssertEqual(dto.signerApps.map(\.scheme), ["nostrsigner://"])
    }

    /// An unrecognised stage token re-types to `.unknown` (forward-compat
    /// fallback), matching the JSON enum's `unknown` case.
    func testTypedNip46OnboardingUnknownStageKindMapsToUnknown() throws {
        let dto = try XCTUnwrap(
            TypedNip46OnboardingDecoder.decode(
                bytes: buildNip46Onboarding(
                    signerApps: [],
                    stageKind: "some_future_stage",
                    progressMessage: nil,
                    isInFlight: false,
                    isFailed: false,
                    isTerminalSuccess: false,
                    canCancel: false)))
        XCTAssertEqual(dto.stageKind, .unknown)
    }

    func testAbsentNip46OnboardingSidecarFallsBack() {
        XCTAssertNil(TypedNip46OnboardingDecoder.decode(from: []))
    }

    func testWrongSchemaNip46OnboardingFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedNip46OnboardingDecoder.key,
            schemaId: "not.nip46_onboarding",
            schemaVersion: 1,
            fileIdentifier: TypedNip46OnboardingDecoder.fileIdentifier,
            payload: buildNip46Onboarding(
                signerApps: [], stageKind: nil, progressMessage: nil,
                isInFlight: false, isFailed: false, isTerminalSuccess: false,
                canCancel: false))
        XCTAssertNil(TypedNip46OnboardingDecoder.decode(from: [envelope]))
    }

    func testGarbledNip46OnboardingBytesFallBack() {
        var garbled = buildNip46Onboarding(
            signerApps: [], stageKind: nil, progressMessage: nil,
            isInFlight: false, isFailed: false, isTerminalSuccess: false,
            canCancel: false)
        garbled[4] = UInt8(ascii: "X")
        XCTAssertNil(TypedNip46OnboardingDecoder.decode(bytes: garbled))
    }

    // MARK: - FlatBuffers builders (direct, via generated builders)

    private func buildBunkerHandshake(
        stage: String,
        message: String?,
        isIdle: Bool,
        isInFlight: Bool,
        isFailed: Bool,
        isTerminalSuccess: Bool,
        canCancel: Bool,
        stageLabel: String
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let stageOff = fbb.create(string: stage)
        let messageOff = message.map { fbb.create(string: $0) } ?? Offset()
        let labelOff = fbb.create(string: stageLabel)
        let root = nmp_kernel_BunkerHandshake.createBunkerHandshake(
            &fbb,
            stageOffset: stageOff,
            hasMessage: message != nil,
            messageOffset: messageOff,
            isIdle: isIdle,
            isInFlight: isInFlight,
            isFailed: isFailed,
            isTerminalSuccess: isTerminalSuccess,
            canCancel: canCancel,
            stageLabelOffset: labelOff)
        nmp_kernel_BunkerHandshake.finish(&fbb, end: root)
        return fbb.data
    }

    private struct SignerAppFixture {
        let scheme: String
        let displayLabel: String
        let signerKind: String
    }

    private func buildNip46Onboarding(
        signerApps: [SignerAppFixture],
        stageKind: String?,
        progressMessage: String?,
        isInFlight: Bool,
        isFailed: Bool,
        isTerminalSuccess: Bool,
        canCancel: Bool
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let appOffsets: [Offset] = signerApps.map { app in
            let schemeOff = fbb.create(string: app.scheme)
            let labelOff = fbb.create(string: app.displayLabel)
            let kindOff = fbb.create(string: app.signerKind)
            return nmp_kernel_SignerApp.createSignerApp(
                &fbb,
                schemeOffset: schemeOff,
                displayLabelOffset: labelOff,
                signerKindOffset: kindOff)
        }
        let appsVec = fbb.createVector(ofOffsets: appOffsets)
        let stageOff = stageKind.map { fbb.create(string: $0) } ?? Offset()
        let progressOff = progressMessage.map { fbb.create(string: $0) } ?? Offset()
        let root = nmp_kernel_Nip46Onboarding.createNip46Onboarding(
            &fbb,
            signerAppsVectorOffset: appsVec,
            hasStageKind: stageKind != nil,
            stageKindOffset: stageOff,
            hasProgressMessage: progressMessage != nil,
            progressMessageOffset: progressOff,
            isInFlight: isInFlight,
            isFailed: isFailed,
            isTerminalSuccess: isTerminalSuccess,
            canCancel: canCancel)
        nmp_kernel_Nip46Onboarding.finish(&fbb, end: root)
        return fbb.data
    }
}
