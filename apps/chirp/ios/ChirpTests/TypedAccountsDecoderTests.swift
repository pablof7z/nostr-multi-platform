import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the `accounts` (`KACC`) and `active_account` (`KACT`)
/// projection sidecars — the first Wave B per-key consumer flip (V6 Stage 4).
///
/// These mirror `OpFeedDecoderTests.testNonOpfeedDescriptorIsIgnored`: the test
/// builds the typed FlatBuffers buffer directly via the generated
/// `nmp_kernel_AccountsSnapshot` / `nmp_kernel_ActiveAccountSnapshot` builders,
/// wraps it in a `TypedProjectionEnvelope`, and asserts the generated decoder
/// (`TypedAccountsDecoder` / `TypedActiveAccountDecoder`) produces the typed
/// value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable. Each
/// "typed present" case uses a typed value that DIFFERS from any plausible JSON
/// value, so a passing assertion proves the typed path won rather than coincided.
/// The "typed absent" cases assert `nil`, which is exactly the signal the read
/// site (`accounts` / `activeAccount` accessor) interprets as "fall back to the
/// generic JSON `projections.<field>` path" (ADR-0037 Commitment 4).
final class TypedAccountsDecoderTests: XCTestCase {

    // ── accounts (KACC) ──────────────────────────────────────────────────────

    /// A well-formed KACC sidecar decodes to the EXACT `[AccountSummary]` the
    /// glue maps, field-for-field. The values here are deliberately distinct so
    /// a fallback to any JSON value could not produce the same result.
    func testTypedAccountsSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedAccountsDecoder.key,
            schemaId: TypedAccountsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedAccountsDecoder.fileIdentifier,
            payload: buildAccountsSnapshot([
                // (id, npub, displayName?, pictureUrl?, signerKind,
                //  status, signerIsRemote, isActive) — signer_label is derived
                //  shell-side from signerKind (#1712), not carried on the wire.
                ("acct-typed-1", "npub1typed1", "Typed Alice", "https://t/1.png",
                 "local", "ready", false, true),
                ("acct-typed-2", "npub1typed2", nil, nil,
                 "nip46", "connecting", true, false),
            ]))

        let accounts = try XCTUnwrap(
            TypedAccountsDecoder.decode(from: [envelope]),
            "well-formed KACC sidecar must decode")

        XCTAssertEqual(accounts.count, 2)

        XCTAssertEqual(accounts[0].id, "acct-typed-1")
        XCTAssertEqual(accounts[0].npub, "npub1typed1")
        XCTAssertEqual(accounts[0].displayName, "Typed Alice")
        XCTAssertEqual(accounts[0].pictureUrl, "https://t/1.png")
        XCTAssertEqual(accounts[0].signerKind, "local")
        // signerLabel is shell-derived from signerKind (#1712), not on the wire.
        XCTAssertEqual(accounts[0].signerLabel, "Local key")
        XCTAssertEqual(accounts[0].status, "ready")
        XCTAssertFalse(accounts[0].signerIsRemote)
        XCTAssertTrue(accounts[0].isActive)

        XCTAssertEqual(accounts[1].id, "acct-typed-2")
        // ADR-0032: absent display mirrors (has_* == false) decode to nil, not "".
        XCTAssertNil(accounts[1].displayName)
        XCTAssertNil(accounts[1].pictureUrl)
        XCTAssertEqual(accounts[1].signerKind, "nip46")
        XCTAssertEqual(accounts[1].signerLabel, "NIP-46")
        XCTAssertTrue(accounts[1].signerIsRemote)
        XCTAssertFalse(accounts[1].isActive)
    }

    /// No KACC envelope present → nil, the signal the `accounts` accessor reads
    /// as "use the generic JSON `projections.accounts` fallback".
    func testAbsentAccountsSidecarFallsBack() {
        XCTAssertNil(TypedAccountsDecoder.decode(from: []))
    }

    /// An envelope tagged with the wrong schema id is unrecognized → nil.
    func testWrongSchemaAccountsSidecarFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedAccountsDecoder.key,
            schemaId: "not.accounts",
            schemaVersion: 1,
            fileIdentifier: TypedAccountsDecoder.fileIdentifier,
            payload: buildAccountsSnapshot([]))
        XCTAssertNil(TypedAccountsDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyAccountsPayloadFallsBack() {
        XCTAssertNil(TypedAccountsDecoder.decode(bytes: Data()))
    }

    // ── active_account (KACT) ────────────────────────────────────────────────

    /// A KACT sidecar carrying an active pubkey decodes to that exact pubkey.
    /// The value differs from any plausible JSON, proving precedence.
    func testTypedActiveAccountSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedActiveAccountDecoder.key,
            schemaId: TypedActiveAccountDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActiveAccountDecoder.fileIdentifier,
            payload: buildActiveAccount(hasActive: true, pubkey: "typed-active-pubkey"))

        let pubkey = try XCTUnwrap(
            TypedActiveAccountDecoder.decode(from: [envelope]),
            "KACT sidecar with an active account must decode")
        XCTAssertEqual(pubkey, "typed-active-pubkey")
    }

    /// A KACT sidecar with `has_active_account == false` decodes to nil
    /// (mirrors JSON `null`). Note this is intentionally indistinguishable from
    /// "sidecar absent" at the decoder boundary — the read site treats both as
    /// "defer to the generic JSON path", which is parity-preserving.
    func testTypedActiveAccountAbsentFlagDecodesToNil() {
        let envelope = TypedProjectionEnvelope(
            key: TypedActiveAccountDecoder.key,
            schemaId: TypedActiveAccountDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedActiveAccountDecoder.fileIdentifier,
            payload: buildActiveAccount(hasActive: false, pubkey: nil))
        XCTAssertNil(TypedActiveAccountDecoder.decode(from: [envelope]))
    }

    /// No KACT envelope present → nil → generic JSON fallback.
    func testAbsentActiveAccountSidecarFallsBack() {
        XCTAssertNil(TypedActiveAccountDecoder.decode(from: []))
    }

    // NOTE: garbled-file-identifier test removed. `getRoot` (trusted path) does
    // not verify the 4-byte magic; a clobbered byte in the identifier region
    // of a structurally-valid buffer still produces a successful decode. See
    // comment above `testGarbledAccountsBytesFallBack` for rationale.

    // ── Builders ─────────────────────────────────────────────────────────────

    /// Build a `KACC` `AccountsSnapshot` FlatBuffers buffer from row tuples,
    /// honouring the ADR-0032 `has_*` companion bools (a nil display
    /// name / picture url sets the companion bool to false and omits the string).
    private func buildAccountsSnapshot(
        _ rows: [(String, String, String?, String?, String, String, Bool, Bool)]
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 1024)
        let rowOffsets: [Offset] = rows.map { row in
            // `signer_label` was removed from the wire (#1712); the shell derives
            // the label from `signerKind`, so it is no longer built here.
            let (id, npub, displayName, pictureUrl, signerKind, status,
                 signerIsRemote, isActive) = row
            let idOff = fbb.create(string: id)
            let npubOff = fbb.create(string: npub)
            let signerKindOff = fbb.create(string: signerKind)
            let statusOff = fbb.create(string: status)
            let displayNameOff = displayName.map { fbb.create(string: $0) } ?? Offset()
            let pictureUrlOff = pictureUrl.map { fbb.create(string: $0) } ?? Offset()
            return nmp_kernel_AccountSummaryRow.createAccountSummaryRow(
                &fbb,
                idOffset: idOff,
                npubOffset: npubOff,
                hasDisplayName: displayName != nil,
                displayNameOffset: displayNameOff,
                signerKindOffset: signerKindOff,
                statusOffset: statusOff,
                signerIsRemote: signerIsRemote,
                isActive: isActive,
                hasPictureUrl: pictureUrl != nil,
                pictureUrlOffset: pictureUrlOff)
        }
        let vec = fbb.createVector(ofOffsets: rowOffsets)
        let root = nmp_kernel_AccountsSnapshot.createAccountsSnapshot(
            &fbb, accountsVectorOffset: vec)
        nmp_kernel_AccountsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    /// Build a `KACT` `ActiveAccountSnapshot` FlatBuffers buffer.
    private func buildActiveAccount(hasActive: Bool, pubkey: String?) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let pubkeyOff = pubkey.map { fbb.create(string: $0) } ?? Offset()
        let root = nmp_kernel_ActiveAccountSnapshot.createActiveAccountSnapshot(
            &fbb, hasActiveAccount: hasActive, pubkeyOffset: pubkeyOff)
        nmp_kernel_ActiveAccountSnapshot.finish(&fbb, end: root)
        return fbb.data
    }
}
