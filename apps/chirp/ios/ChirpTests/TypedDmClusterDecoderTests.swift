import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the NIP-17 DM cluster sidecars:
/// `nmp.nip17.dm_inbox` (`NDMI`) and `nmp.nip17.dm_relay_list` (`NDRL`).
///
/// These mirror `TypedAppProjectionsDecoderTests` / `TypedProfileClusterDecoderTests`:
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
/// `has_*` companion-bool semantics (`hasReplyTo` on a DM message,
/// `hasActivePubkey` on the relay list) are pinned to map `false → nil`,
/// reproducing the JSON `null`-when-`None` shape regardless of the empty string
/// slot.
final class TypedDmClusterDecoderTests: XCTestCase {

    // MARK: - nmp.nip17.dm_inbox (NDMI)

    func testDmInboxSidecarIdentityIsExact() {
        XCTAssertEqual(TypedDmInboxDecoder.key, "nmp.nip17.dm_inbox")
        XCTAssertEqual(TypedDmInboxDecoder.schemaId, "nmp.nip17.dm_inbox")
        XCTAssertEqual(TypedDmInboxDecoder.fileIdentifier, "NDMI")
    }

    func testTypedDmInboxSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedDmInboxDecoder.key,
            schemaId: TypedDmInboxDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedDmInboxDecoder.fileIdentifier,
            payload: buildDmInbox(
                conversations: [
                    DmConvoFixture(
                        peerPubkey: "typedpeerA",
                        messages: [
                            DmMsgFixture(
                                id: "typedmsg1", senderPubkey: "typedpeerA",
                                content: "typed hello", createdAt: 1_700_000_001,
                                replyTo: nil, isOutgoing: false,
                                sourceRelays: ["wss://typed.relay/a"]),
                            DmMsgFixture(
                                id: "typedmsg2", senderPubkey: "typedlocal",
                                content: "typed reply", createdAt: 1_700_000_002,
                                replyTo: "typedmsg1", isOutgoing: true,
                                sourceRelays: ["wss://typed.relay/b", "wss://typed.relay/c"]),
                        ]),
                    DmConvoFixture(peerPubkey: "typedpeerB", messages: []),
                ],
                decryptState: "ok"))

        let snap = try XCTUnwrap(
            TypedDmInboxDecoder.decode(from: [envelope]),
            "well-formed NDMI sidecar must decode")

        // Conversation order preserved verbatim (Rust owns it; shell never re-sorts).
        XCTAssertEqual(snap.conversations.map(\.peerPubkey), ["typedpeerA", "typedpeerB"])
        XCTAssertEqual(snap.decryptState, "ok")
        XCTAssertEqual(snap.undecryptedCount, 0)

        let convoA = snap.conversations[0]
        XCTAssertEqual(convoA.messages.map(\.id), ["typedmsg1", "typedmsg2"])
        XCTAssertEqual(convoA.messages[0].content, "typed hello")
        XCTAssertEqual(convoA.messages[0].createdAt, 1_700_000_001)
        XCTAssertFalse(convoA.messages[0].isOutgoing)
        XCTAssertEqual(convoA.messages[0].sourceRelays, ["wss://typed.relay/a"])
        // `has_reply_to == false` → nil; the populated reply maps verbatim.
        XCTAssertNil(convoA.messages[0].replyTo)
        XCTAssertEqual(convoA.messages[1].replyTo, "typedmsg1")
        XCTAssertTrue(convoA.messages[1].isOutgoing)
        XCTAssertEqual(
            convoA.messages[1].sourceRelays,
            ["wss://typed.relay/b", "wss://typed.relay/c"])

        XCTAssertEqual(snap.conversations[1].messages, [])
    }

    /// ADR-0050 §D7 — the `decrypt_state` tri-state + `undecrypted_count`
    /// round-trip through the typed NDMI wire. "limited" (bunker backfill
    /// pending/throttled by the bounded per-account decrypt queue) carries a
    /// non-zero count; "unavailable" (no active account) carries zero.
    func testTypedDmInboxLimitedStateDecodes() throws {
        let snap = try XCTUnwrap(
            TypedDmInboxDecoder.decode(
                bytes: buildDmInbox(conversations: [], decryptState: "limited", undecryptedCount: 7)))
        XCTAssertEqual(snap.decryptState, "limited")
        XCTAssertEqual(snap.undecryptedCount, 7)
        XCTAssertEqual(snap.conversations, [])
    }

    func testTypedDmInboxUnavailableStateDecodes() throws {
        let snap = try XCTUnwrap(
            TypedDmInboxDecoder.decode(
                bytes: buildDmInbox(conversations: [], decryptState: "unavailable")))
        XCTAssertEqual(snap.decryptState, "unavailable")
        XCTAssertEqual(snap.undecryptedCount, 0)
        XCTAssertEqual(snap.conversations, [])
    }

    func testAbsentDmInboxSidecarFallsBack() {
        XCTAssertNil(TypedDmInboxDecoder.decode(from: []))
    }

    func testWrongSchemaDmInboxFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedDmInboxDecoder.key,
            schemaId: "not.dm_inbox",
            schemaVersion: 1,
            fileIdentifier: TypedDmInboxDecoder.fileIdentifier,
            payload: buildDmInbox(conversations: [], decryptState: "ok"))
        XCTAssertNil(TypedDmInboxDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // MARK: - nmp.nip17.dm_relay_list (NDRL)

    func testDmRelayListSidecarIdentityIsExact() {
        XCTAssertEqual(TypedDmRelayListDecoder.key, "nmp.nip17.dm_relay_list")
        XCTAssertEqual(TypedDmRelayListDecoder.schemaId, "nmp.nip17.dm_relay_list")
        XCTAssertEqual(TypedDmRelayListDecoder.fileIdentifier, "NDRL")
    }

    func testTypedDmRelayListSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedDmRelayListDecoder.key,
            schemaId: TypedDmRelayListDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedDmRelayListDecoder.fileIdentifier,
            payload: buildDmRelayList(
                activePubkey: "typedactivepk",
                readRelayUrls: ["wss://typed.dm/read1", "wss://typed.dm/read2"]))

        let snap = try XCTUnwrap(
            TypedDmRelayListDecoder.decode(from: [envelope]),
            "well-formed NDRL sidecar must decode")

        XCTAssertEqual(snap.activePubkey, "typedactivepk")
        // Order preserved verbatim (Rust owns it).
        XCTAssertEqual(snap.readRelayUrls, ["wss://typed.dm/read1", "wss://typed.dm/read2"])
    }

    /// `has_active_pubkey == false` (no account loaded) → nil active pubkey,
    /// reproducing the JSON `null` shape.
    func testTypedDmRelayListAbsentActivePubkeyMapsToNil() throws {
        let snap = try XCTUnwrap(
            TypedDmRelayListDecoder.decode(
                bytes: buildDmRelayList(activePubkey: nil, readRelayUrls: [])))
        XCTAssertNil(snap.activePubkey)
        XCTAssertEqual(snap.readRelayUrls, [])
    }

    func testAbsentDmRelayListSidecarFallsBack() {
        XCTAssertNil(TypedDmRelayListDecoder.decode(from: []))
    }

    func testWrongSchemaDmRelayListFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedDmRelayListDecoder.key,
            schemaId: "not.dm_relay_list",
            schemaVersion: 1,
            fileIdentifier: TypedDmRelayListDecoder.fileIdentifier,
            payload: buildDmRelayList(activePubkey: "pk", readRelayUrls: []))
        XCTAssertNil(TypedDmRelayListDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // MARK: - FlatBuffers builders (direct, via generated builders)

    private struct DmMsgFixture {
        let id: String
        let senderPubkey: String
        let content: String
        let createdAt: UInt64
        let replyTo: String?
        let isOutgoing: Bool
        let sourceRelays: [String]
    }

    private struct DmConvoFixture {
        let peerPubkey: String
        let messages: [DmMsgFixture]
    }

    private func buildDmInbox(
        conversations: [DmConvoFixture],
        decryptState: String,
        undecryptedCount: UInt32 = 0
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let convoOffsets: [Offset] = conversations.map { convo in
            let msgOffsets: [Offset] = convo.messages.map { msg in
                let idOff = fbb.create(string: msg.id)
                let senderOff = fbb.create(string: msg.senderPubkey)
                let contentOff = fbb.create(string: msg.content)
                let replyOff = msg.replyTo.map { fbb.create(string: $0) } ?? Offset()
                let relaysOff = fbb.createVector(
                    ofOffsets: msg.sourceRelays.map { fbb.create(string: $0) })
                return nmp_nip17_DmMessage.createDmMessage(
                    &fbb,
                    idOffset: idOff,
                    senderPubkeyOffset: senderOff,
                    contentOffset: contentOff,
                    createdAt: msg.createdAt,
                    hasReplyTo: msg.replyTo != nil,
                    replyToOffset: replyOff,
                    isOutgoing: msg.isOutgoing,
                    sourceRelaysVectorOffset: relaysOff)
            }
            let peerOff = fbb.create(string: convo.peerPubkey)
            let msgsVec = fbb.createVector(ofOffsets: msgOffsets)
            return nmp_nip17_DmConversation.createDmConversation(
                &fbb, peerPubkeyOffset: peerOff, messagesVectorOffset: msgsVec)
        }
        let convosVec = fbb.createVector(ofOffsets: convoOffsets)
        let decryptStateOff = fbb.create(string: decryptState)
        let root = nmp_nip17_DmInboxSnapshot.createDmInboxSnapshot(
            &fbb,
            conversationsVectorOffset: convosVec,
            decryptStateOffset: decryptStateOff,
            undecryptedCount: undecryptedCount)
        nmp_nip17_DmInboxSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildDmRelayList(activePubkey: String?, readRelayUrls: [String]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let activeOff = activePubkey.map { fbb.create(string: $0) } ?? Offset()
        let urlsVec = fbb.createVector(ofOffsets: readRelayUrls.map { fbb.create(string: $0) })
        let root = nmp_nip17_DmRelayListSnapshot.createDmRelayListSnapshot(
            &fbb,
            hasActivePubkey: activePubkey != nil,
            activePubkeyOffset: activeOff,
            readRelayUrlsVectorOffset: urlsVec)
        nmp_nip17_DmRelayListSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

}
