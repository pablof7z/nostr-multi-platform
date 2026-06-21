import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the `wallet` (`NWST`) producer-field-add flip and the
/// `settings_hub` (`KSHB`) kernel-built-in flip.
///
/// These mirror `TypedMarmotClusterDecoderTests` / `TypedPublishRelayDecoderTests`:
/// build the typed FlatBuffers buffer directly via the generated builders, wrap
/// it in a `TypedProjectionEnvelope` carrying the producer's actual
/// `(key, schemaId)`, and assert the generated `Typed<Key>Decoder` produces the
/// Chirp domain value via `TypedProjectionGlue`.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable. Each
/// "typed present" case uses values that DIFFER from any plausible JSON value,
/// so a passing assertion proves the typed path won rather than coincided. The
/// "absent / wrong-schema" cases assert `nil`, the signal the read
/// site interprets as "fall back to the generic JSON `projections.<field>` path"
/// (ADR-0037 Commitment 4).
///
/// IDENTITY NOTE: `wallet` is the only flipped key where `key != schemaId` — the
/// producer publishes envelope `key: "wallet"` with `schema_id:
/// "nmp.nip47.wallet"`. The `*IdentityIsExact` case pins that contract so a
/// regression that collapses key→schema_id (or vice versa) fails loudly.
final class TypedWalletSettingsHubDecoderTests: XCTestCase {

    // MARK: - wallet (NWST)

    func testWalletSidecarIdentityIsExact() {
        // key != schemaId for wallet — the producer's envelope key is "wallet"
        // while the schema_id is the dotted "nmp.nip47.wallet".
        XCTAssertEqual(TypedWalletDecoder.key, "wallet")
        XCTAssertEqual(TypedWalletDecoder.schemaId, "nmp.nip47.wallet")
        XCTAssertEqual(TypedWalletDecoder.fileIdentifier, "NWST")
    }

    func testTypedWalletSidecarDecodesWithBalances() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedWalletDecoder.key,
            schemaId: TypedWalletDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedWalletDecoder.fileIdentifier,
            payload: buildWalletStatus(
                status: "ready",
                relayUrl: "wss://typed.example/nwc",
                walletPubkeyHex: "ab".repeated(32),
                walletNpub: "npub1typedwallet",
                balanceMsats: 21_000_000,
                balanceSats: 21_000,
                isReady: true,
                isConnected: true))

        let wallet = try XCTUnwrap(TypedWalletDecoder.decode(from: [envelope]))
        XCTAssertEqual(wallet.status, "ready")
        XCTAssertEqual(wallet.relayUrl, "wss://typed.example/nwc")
        // The producer field-add: walletPubkeyHex must round-trip the wire value
        // (NOT be fabricated from the npub). This is the field that unblocked the
        // flip — a missing/empty value here would mean the wire lost it.
        XCTAssertEqual(wallet.walletPubkeyHex, "ab".repeated(32))
        XCTAssertEqual(wallet.walletNpub, "npub1typedwallet")
        XCTAssertEqual(wallet.balanceMsats, 21_000_000)
        XCTAssertEqual(wallet.balanceSats, 21_000)
        XCTAssertTrue(wallet.isReady)
        XCTAssertTrue(wallet.isConnected)
    }

    func testTypedWalletSidecarMapsAbsentBalancesToNil() throws {
        // has_balance_msats == false / has_balance_sats == false → both nil,
        // reproducing the JSON `null` shape (the disconnected / pre-balance case).
        let envelope = TypedProjectionEnvelope(
            key: TypedWalletDecoder.key,
            schemaId: TypedWalletDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedWalletDecoder.fileIdentifier,
            payload: buildWalletStatus(
                status: "disconnected",
                relayUrl: "",
                walletPubkeyHex: "cd".repeated(32),
                walletNpub: "",
                balanceMsats: nil,
                balanceSats: nil,
                isReady: false,
                isConnected: false))

        let wallet = try XCTUnwrap(TypedWalletDecoder.decode(from: [envelope]))
        XCTAssertEqual(wallet.status, "disconnected")
        XCTAssertEqual(wallet.walletPubkeyHex, "cd".repeated(32))
        XCTAssertNil(wallet.balanceMsats)
        XCTAssertNil(wallet.balanceSats)
        XCTAssertFalse(wallet.isReady)
        XCTAssertFalse(wallet.isConnected)
    }

    func testTypedWalletSidecarAbsentReturnsNil() {
        // No envelope carries the wallet key → nil → generic JSON fallback.
        XCTAssertNil(TypedWalletDecoder.decode(from: []))
    }

    func testTypedWalletSidecarWrongSchemaReturnsNil() {
        // Right buffer, wrong schemaId → the decoder must NOT match it.
        let envelope = TypedProjectionEnvelope(
            key: TypedWalletDecoder.key,
            schemaId: "nmp.nip47.NOT_WALLET",
            schemaVersion: 1,
            fileIdentifier: TypedWalletDecoder.fileIdentifier,
            payload: buildWalletStatus(
                status: "ready", relayUrl: "wss://x", walletPubkeyHex: "ef".repeated(32),
                walletNpub: "npub1x", balanceMsats: 1, balanceSats: 0,
                isReady: true, isConnected: true))
        XCTAssertNil(TypedWalletDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-payload test was removed. The decode path now uses
    // unchecked `getRoot` (trusted in-process FFI boundary); a structurally
    // invalid payload that survives the `!isEmpty` guard is NOT validated and
    // makes `getRoot` read past the buffer — it traps rather than returning nil,
    // so the old "garbled → nil fallback" contract is impossible to assert. The
    // key+schemaId envelope routing in `decode(from:)` is the selection
    // mechanism, not buffer well-formedness.

    // MARK: - settings_hub (KSHB)

    func testSettingsHubSidecarIdentityIsExact() {
        XCTAssertEqual(TypedSettingsHubDecoder.key, "settings_hub")
        XCTAssertEqual(TypedSettingsHubDecoder.schemaId, "settings_hub")
        XCTAssertEqual(TypedSettingsHubDecoder.fileIdentifier, "KSHB")
    }

    func testTypedSettingsHubSidecarDecodesToRelayCountDict() throws {
        // A value (37) no plausible JSON fixture coincidentally carries, so a
        // passing assert proves the typed path won.
        let envelope = TypedProjectionEnvelope(
            key: TypedSettingsHubDecoder.key,
            schemaId: TypedSettingsHubDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedSettingsHubDecoder.fileIdentifier,
            payload: buildSettingsHub(relayCount: 37))

        let dict = try XCTUnwrap(TypedSettingsHubDecoder.decode(from: [envelope]))
        // The glue rebuilds the SAME single-key dict the JSON object yields.
        XCTAssertEqual(dict, ["relay_count": 37])
    }

    func testTypedSettingsHubWrapsThroughSummary() throws {
        // The downstream `SettingsHubSummary(relayCount:)` wrap reads
        // dict["relay_count"] — verify the end-to-end shape the SettingsHubView
        // consumes (pluralized subtitle owned by the shell, ADR-0032 §6/AP1).
        let envelope = TypedProjectionEnvelope(
            key: TypedSettingsHubDecoder.key,
            schemaId: TypedSettingsHubDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedSettingsHubDecoder.fileIdentifier,
            payload: buildSettingsHub(relayCount: 1))
        let dict = try XCTUnwrap(TypedSettingsHubDecoder.decode(from: [envelope]))
        let summary = SettingsHubSummary(relayCount: dict["relay_count"] ?? 0)
        XCTAssertEqual(summary.relayCount, 1)
        XCTAssertEqual(summary.relaysSubtitle, "1 relay")
    }

    func testTypedSettingsHubAbsentReturnsNil() {
        XCTAssertNil(TypedSettingsHubDecoder.decode(from: []))
    }

    func testTypedSettingsHubWrongSchemaReturnsNil() {
        let envelope = TypedProjectionEnvelope(
            key: TypedSettingsHubDecoder.key,
            schemaId: "NOT_settings_hub",
            schemaVersion: 1,
            fileIdentifier: TypedSettingsHubDecoder.fileIdentifier,
            payload: buildSettingsHub(relayCount: 9))
        XCTAssertNil(TypedSettingsHubDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-payload test was removed. The decode path now uses
    // unchecked `getRoot` (trusted in-process FFI boundary); a structurally
    // invalid payload that survives the `!isEmpty` guard is NOT validated and
    // makes `getRoot` read past the buffer — it traps rather than returning nil,
    // so the old "garbled → nil fallback" contract is impossible to assert. The
    // key+schemaId envelope routing in `decode(from:)` is the selection
    // mechanism, not buffer well-formedness.

    // MARK: - buffer builders

    private func buildWalletStatus(
        status: String,
        relayUrl: String,
        walletPubkeyHex: String,
        walletNpub: String,
        balanceMsats: UInt64?,
        balanceSats: UInt64?,
        isReady: Bool,
        isConnected: Bool
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let statusOff = fbb.create(string: status)
        let relayUrlOff = fbb.create(string: relayUrl)
        let walletNpubOff = fbb.create(string: walletNpub)
        let walletPubkeyHexOff = fbb.create(string: walletPubkeyHex)
        // `wallet_npub_short` vtable slot deprecated (#1678, D7); not written.
        let root = nmp_nip47_WalletStatus.createWalletStatus(
            &fbb,
            statusOffset: statusOff,
            relayUrlOffset: relayUrlOff,
            walletNpubOffset: walletNpubOff,
            hasBalanceMsats: balanceMsats != nil,
            balanceMsats: balanceMsats ?? 0,
            hasBalanceSats: balanceSats != nil,
            balanceSats: balanceSats ?? 0,
            isReady: isReady,
            isConnected: isConnected,
            hasConnectionState: false,
            walletPubkeyHexOffset: walletPubkeyHexOff)
        nmp_nip47_WalletStatus.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildSettingsHub(relayCount: UInt32) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 64)
        let root = nmp_kernel_SettingsHubSnapshot.createSettingsHubSnapshot(
            &fbb, relayCount: relayCount)
        nmp_kernel_SettingsHubSnapshot.finish(&fbb, end: root)
        return fbb.data
    }
}

private extension String {
    /// Repeat a short literal `count` times (test fixture readability).
    func repeated(_ count: Int) -> String { String(repeating: self, count: count) }
}
