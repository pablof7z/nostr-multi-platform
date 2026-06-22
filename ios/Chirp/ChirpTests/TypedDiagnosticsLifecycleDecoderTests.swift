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
        // Raw decoded fields (aim.md §62: no pre-formatted strings on wire).
        // #1768 — no semantic tone on the wire; the shell derives its own hue
        // from these raw tokens via `DiagnosticsTone`.
        XCTAssertEqual(row.role, "content")
        XCTAssertEqual(row.connection, "connected")
        XCTAssertEqual(row.auth, "ok")
        // Shell-side computed display labels derived from the raw fields above.
        XCTAssertEqual(row.shortUrl, "typed-diag.example")
        XCTAssertEqual(row.roleLabel, "Content")
        XCTAssertEqual(row.connectionLabel, "Connected")
        XCTAssertEqual(row.authLabel, "Ok")
        XCTAssertEqual(row.totalSubCount, 7)
        XCTAssertEqual(row.activeSubCount, 5)
        XCTAssertEqual(row.eosedSubCount, 3)
        XCTAssertEqual(row.totalEventsRx, 4242)
        XCTAssertEqual(row.totalEventsDisplay, "4.2K")
        XCTAssertEqual(row.reconnectCount, 2)
        // Raw byte counters; the shell renders `bytesRxDisplay` (> 0 → string)
        // and `bytesTxDisplay` (0 → nil) from them.
        XCTAssertEqual(row.bytesRx, 12_288)
        XCTAssertEqual(row.bytesTx, 0)
        // Lock the exact rendered byte label (12288 / 1024 = 12.0 KB) so the
        // `KB`-vs-`KiB` cross-shell parity can't silently regress.
        XCTAssertEqual(row.bytesRxDisplay, "12.0 KB")
        XCTAssertNil(row.bytesTxDisplay)
        // Raw discovery kind numbers; the shell derives `discoveryKindsLabel`.
        XCTAssertEqual(row.discoveryKinds, [0, 10002])
        XCTAssertEqual(row.discoveryKindsLabel, "profile (0), relay-list (10002)")
        // Raw Unix-ms timestamps; shells format as "Xs ago" at render time.
        XCTAssertEqual(row.lastConnectedMs, 1_700_000_003_000)
        XCTAssertEqual(row.lastEventMs, 0)
        XCTAssertNil(row.lastNotice)
        XCTAssertEqual(row.lastError, "typed boom")

        XCTAssertEqual(row.wireSubs.count, 1)
        let sub = row.wireSubs[0]
        XCTAssertEqual(sub.wireId, "typed-wire-1")
        XCTAssertEqual(sub.relayUrl, "wss://typed-diag.example")
        XCTAssertEqual(sub.filterSummary, "typed filter")
        // Raw decoded fields + shell-side computed labels.
        XCTAssertEqual(sub.state, "open")
        XCTAssertEqual(sub.stateLabel, "Open")
        XCTAssertEqual(sub.consumerCount, 1)
        XCTAssertEqual(sub.consumerCountLabel, "1 consumer")
        XCTAssertEqual(sub.eventsRx, 34)
        XCTAssertEqual(sub.eventsRxDisplay, "34")
        XCTAssertTrue(sub.eoseObserved)
        // Raw Unix-ms timestamps — rendered by the shell at display time.
        XCTAssertEqual(sub.openedMs, 1_700_000_060_000)
        XCTAssertEqual(sub.lastEventMs, 0)
        XCTAssertEqual(sub.eoseMs, 0)
        XCTAssertNil(sub.closeReason)

        // ADR-0051: this row carries no `info` child table (the `info: null`
        // case — no NIP-11 document fetched yet), so it decodes to nil. Table
        // presence is the discriminator (no `has_info` flag).
        XCTAssertNil(row.info)

        XCTAssertEqual(snap.interests.count, 1)
        let interest = snap.interests[0]
        XCTAssertEqual(interest.key, "typed-interest")
        XCTAssertEqual(interest.state, "Typed Active")
        XCTAssertEqual(interest.refcount, 3)
        XCTAssertEqual(interest.cacheCoverage, "typed 80%")
        XCTAssertEqual(interest.relayUrls, ["wss://typed-a", "wss://typed-b"])
    }

    /// ADR-0051: a row carrying a fully-populated NIP-11 `info` child table
    /// decodes field-for-field (name/description/icon/pubkey/contact/software/
    /// version), the `supported_nips` uint vector, and the three tri-state
    /// limitation flags. Values DIFFER from any plausible default so a pass
    /// proves the typed `info` path wired through.
    func testTypedRelayDiagnosticsInfoDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayDiagnosticsDecoder.key,
            schemaId: TypedRelayDiagnosticsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayDiagnosticsDecoder.fileIdentifier,
            payload: buildRelayDiagnosticsWithInfo(full: true))
        let snap = try XCTUnwrap(TypedRelayDiagnosticsDecoder.decode(from: [envelope]))
        let info = try XCTUnwrap(snap.relays[0].info, "populated info table must decode")
        XCTAssertEqual(info.name, "Typed Relay")
        XCTAssertEqual(info.description, "Typed description")
        XCTAssertEqual(info.icon, "https://typed.example/icon.png")
        XCTAssertEqual(info.pubkey, "typed-pubkey-hex")
        XCTAssertEqual(info.contact, "typed@example.com")
        XCTAssertEqual(info.software, "typed-strfry")
        XCTAssertEqual(info.version, "9.9.9-typed")
        XCTAssertEqual(info.supportedNips, [1, 11, 42])
        XCTAssertEqual(info.paymentRequired, true)
        XCTAssertEqual(info.authRequired, false)
        XCTAssertEqual(info.restrictedWrites, true)
    }

    /// `has_* == false` (and absent limitation presence bits) lift to nil,
    /// byte-faithful to the JSON `null`. Only `name` + `auth_required` advertised.
    func testTypedRelayDiagnosticsPartialInfoLeavesAbsentFieldsNil() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedRelayDiagnosticsDecoder.key,
            schemaId: TypedRelayDiagnosticsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedRelayDiagnosticsDecoder.fileIdentifier,
            payload: buildRelayDiagnosticsWithInfo(full: false))
        let snap = try XCTUnwrap(TypedRelayDiagnosticsDecoder.decode(from: [envelope]))
        let info = try XCTUnwrap(snap.relays[0].info)
        XCTAssertEqual(info.name, "Minimal Typed Relay")
        XCTAssertNil(info.description)
        XCTAssertNil(info.icon)
        XCTAssertNil(info.pubkey)
        XCTAssertNil(info.contact)
        XCTAssertNil(info.software)
        XCTAssertNil(info.version)
        XCTAssertEqual(info.supportedNips, [])
        XCTAssertNil(info.paymentRequired)
        XCTAssertEqual(info.authRequired, true)
        XCTAssertNil(info.restrictedWrites)
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

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

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

    // ── Builders (relay_diagnostics) ─────────────────────────────────────────

    /// Build a KRDG buffer with one fully-populated relay row (one `has_*`
    /// present, one absent — to prove the nil-vs-empty distinction), one nested
    /// wire-sub, and one logical-interest row.
    private func buildRelayDiagnostics() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 2048)

        // Nested wire-sub: raw `state` / `consumer_count` / `events_rx`; the
        // shell derives `stateLabel` / `consumerCountLabel` / `eventsRxDisplay`
        // from these. `last_event` / `eose` / `close_reason` absent (zero
        // sentinel = "never observed"). aim.md §62: raw values on wire, no
        // pre-formatted strings.
        let subWireId = fbb.create(string: "typed-wire-1")
        let subRelayUrl = fbb.create(string: "wss://typed-diag.example")
        let subFilter = fbb.create(string: "typed filter")
        let subState = fbb.create(string: "open")
        let sub = nmp_kernel_RelayDiagnosticsWireSub.createRelayDiagnosticsWireSub(
            &fbb,
            wireIdOffset: subWireId,
            relayUrlOffset: subRelayUrl,
            filterSummaryOffset: subFilter,
            stateOffset: subState,
            consumerCount: 1,
            eventsRx: 34,
            eoseObserved: true,
            openedMs: 1_700_000_060_000,
            lastEventMs: 0,
            eoseMs: 0,
            hasCloseReason: false)
        let wireSubsVec = fbb.createVector(ofOffsets: [sub])

        // Relay row: raw `role` / `connection` / `auth` strings; raw `bytes_rx`
        // counter (> 0 so the shell's `bytesRxDisplay` renders); `bytes_tx` = 0
        // (→ `bytesTxDisplay` nil). `last_connected` / `last_error` present;
        // `last_event` / `last_notice` absent (zero = never). `discovery_kinds`
        // carries raw kind numbers the shell renders via `discoveryKindsLabel`.
        let relayUrl = fbb.create(string: "wss://typed-diag.example")
        let role = fbb.create(string: "content")
        let connection = fbb.create(string: "connected")
        let auth = fbb.create(string: "ok")
        let lastError = fbb.create(string: "typed boom")
        let discoveryKindsVec = fbb.createVector([UInt64(0), UInt64(10002)])
        let row = nmp_kernel_RelayDiagnosticsRow.createRelayDiagnosticsRow(
            &fbb,
            relayUrlOffset: relayUrl,
            roleOffset: role,
            connectionOffset: connection,
            authOffset: auth,
            totalSubCount: 7,
            activeSubCount: 5,
            eosedSubCount: 3,
            totalEventsRx: 4242,
            reconnectCount: 2,
            bytesRx: 12_288,
            bytesTx: 0,
            lastConnectedMs: 1_700_000_003_000,
            lastEventMs: 0,
            hasLastNotice: false,
            hasLastError: true,
            lastErrorOffset: lastError,
            wireSubsVectorOffset: wireSubsVec,
            discoveryKindsVectorOffset: discoveryKindsVec)
        let relaysVec = fbb.createVector(ofOffsets: [row])

        // Interest row with a 2-element relay-url string vector.
        let iKey = fbb.create(string: "typed-interest")
        let iState = fbb.create(string: "Typed Active")
        let iCoverage = fbb.create(string: "typed 80%")
        let urlA = fbb.create(string: "wss://typed-a")
        let urlB = fbb.create(string: "wss://typed-b")
        let urlsVec = fbb.createVector(ofOffsets: [urlA, urlB])
        let interest = nmp_kernel_RelayDiagnosticsInterest.createRelayDiagnosticsInterest(
            &fbb,
            keyOffset: iKey,
            stateOffset: iState,
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

    /// Build a KRDG buffer with one relay row carrying an ADR-0051 NIP-11 `info`
    /// child table. `full == true` populates every field; `full == false` carries
    /// only `name` + `auth_required` (the rest `has_* == false` → nil).
    private func buildRelayDiagnosticsWithInfo(full: Bool) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 2048)

        let info: Offset
        if full {
            let iName = fbb.create(string: "Typed Relay")
            let iDesc = fbb.create(string: "Typed description")
            let iIcon = fbb.create(string: "https://typed.example/icon.png")
            let iPubkey = fbb.create(string: "typed-pubkey-hex")
            let iContact = fbb.create(string: "typed@example.com")
            let iSoftware = fbb.create(string: "typed-strfry")
            let iVersion = fbb.create(string: "9.9.9-typed")
            let nipsVec = fbb.createVector([UInt32(1), UInt32(11), UInt32(42)])
            info = nmp_kernel_RelayDiagnosticsInfo.createRelayDiagnosticsInfo(
                &fbb,
                hasName: true, nameOffset: iName,
                hasDescription: true, descriptionOffset: iDesc,
                hasIcon: true, iconOffset: iIcon,
                hasPubkey: true, pubkeyOffset: iPubkey,
                hasContact: true, contactOffset: iContact,
                hasSoftware: true, softwareOffset: iSoftware,
                hasVersion: true, versionOffset: iVersion,
                supportedNipsVectorOffset: nipsVec,
                hasPaymentRequired: true, paymentRequired: true,
                hasAuthRequired: true, authRequired: false,
                hasRestrictedWrites: true, restrictedWrites: true)
        } else {
            let iName = fbb.create(string: "Minimal Typed Relay")
            info = nmp_kernel_RelayDiagnosticsInfo.createRelayDiagnosticsInfo(
                &fbb,
                hasName: true, nameOffset: iName,
                hasAuthRequired: true, authRequired: true)
        }

        let relayUrl = fbb.create(string: "wss://typed-info.example")
        let role = fbb.create(string: "content")
        let connection = fbb.create(string: "connected")
        let auth = fbb.create(string: "ok")
        let row = nmp_kernel_RelayDiagnosticsRow.createRelayDiagnosticsRow(
            &fbb,
            relayUrlOffset: relayUrl,
            roleOffset: role,
            connectionOffset: connection,
            authOffset: auth,
            totalSubCount: 0,
            activeSubCount: 0,
            eosedSubCount: 0,
            totalEventsRx: 0,
            reconnectCount: 0,
            bytesRx: 0,
            bytesTx: 0,
            lastConnectedMs: 0,
            lastEventMs: 0,
            hasLastNotice: false,
            hasLastError: false,
            infoOffset: info)
        let relaysVec = fbb.createVector(ofOffsets: [row])

        let root = nmp_kernel_RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(
            &fbb, relaysVectorOffset: relaysVec)
        nmp_kernel_RelayDiagnosticsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

}
