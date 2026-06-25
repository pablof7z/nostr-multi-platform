import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the Marmot (MLS-over-Nostr) push-projection cluster
/// (V-107 / ADR-0039): `nmp.marmot.snapshot` (`NMMS`) and
/// `nmp.marmot.messages` (`NMMG`).
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
/// `has_*` companion-bool semantics (`hasUnreadCount`/`hasLastMsgAt` on a group,
/// `hasDTag`/`hasAgeSecs` on the key package,
/// `hasEpoch` on a message) are pinned to
/// map `false → nil`, reproducing the JSON `null`-when-`None` shape regardless of
/// the empty value slot — the parity the `MarmotStore.apply` `Equatable` compare
/// depends on. `invitesChipLabel` and `displayName`/`initials` are shell-computed
/// (aim.md §2) and are no longer wire fields (schema v4+).
final class TypedMarmotClusterDecoderTests: XCTestCase {

    // MARK: - nmp.marmot.snapshot (NMMS)

    func testMarmotSnapshotSidecarIdentityIsExact() {
        XCTAssertEqual(TypedMarmotSnapshotDecoder.key, "nmp.marmot.snapshot")
        XCTAssertEqual(TypedMarmotSnapshotDecoder.schemaId, "nmp.marmot.snapshot")
        XCTAssertEqual(TypedMarmotSnapshotDecoder.fileIdentifier, "NMMS")
    }

    func testTypedMarmotSnapshotSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedMarmotSnapshotDecoder.key,
            schemaId: TypedMarmotSnapshotDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedMarmotSnapshotDecoder.fileIdentifier,
            payload: buildMarmotSnapshot(
                groups: [
                    GroupFixture(
                        idHex: "typedgroupA", name: "Typed Group A",
                        members: ["typedmemberA1", "typedmemberA2"],
                        memberCount: 2,
                        unreadCount: 7, lastMsgAt: 1_700_000_111),
                    // `has_unread_count == false` / `has_last_msg_at == false`
                    // → both must surface nil (the JSON `null` shape).
                    GroupFixture(
                        idHex: "typedgroupB", name: "",
                        members: [], memberCount: 0,
                        unreadCount: nil, lastMsgAt: nil),
                ],
                pendingWelcomes: [
                    WelcomeFixture(
                        idHex: "typedwelcome1", groupName: "Typed Invite",
                        inviterNpub: "typedinviterpk"),
                ],
                keyPackage: KeyPackageFixture(
                    published: true, dTag: "typed-d-tag", ageSecs: 4242,
                    stale: false, isRegistered: true),
                cachedKpPubkeys: ["typedcached1", "typedcached2"],
                isRegistered: true))

        let snap = try XCTUnwrap(
            TypedMarmotSnapshotDecoder.decode(from: [envelope]),
            "well-formed NMMS sidecar must decode")

        // Group order preserved verbatim (Rust owns it; the shell never re-sorts).
        XCTAssertEqual(snap.groups.map(\.idHex), ["typedgroupA", "typedgroupB"])
        let groupA = snap.groups[0]
        XCTAssertEqual(groupA.name, "Typed Group A")
        XCTAssertEqual(groupA.displayName, "Typed Group A")
        XCTAssertEqual(groupA.initials, "TY")
        XCTAssertEqual(groupA.members, ["typedmemberA1", "typedmemberA2"])
        XCTAssertEqual(groupA.memberCount, 2)
        XCTAssertEqual(groupA.unreadCount, 7)
        XCTAssertEqual(groupA.lastMsgAt, 1_700_000_111)

        // `has_*` companion false → nil (NOT 0 / empty), byte-identical to JSON `null`.
        let groupB = snap.groups[1]
        XCTAssertEqual(groupB.displayName, "Untitled group")
        XCTAssertEqual(groupB.members, [])
        XCTAssertEqual(groupB.memberCount, 0)
        XCTAssertNil(groupB.unreadCount)
        XCTAssertNil(groupB.lastMsgAt)

        XCTAssertEqual(snap.pendingWelcomes.map(\.idHex), ["typedwelcome1"])
        XCTAssertEqual(snap.pendingWelcomes[0].displayName, "Typed Invite")
        XCTAssertEqual(snap.pendingWelcomes[0].inviterNpub, "typedinviterpk")

        XCTAssertTrue(snap.keyPackage.published)
        XCTAssertEqual(snap.keyPackage.dTag, "typed-d-tag")
        XCTAssertEqual(snap.keyPackage.ageSecs, 4242)
        XCTAssertFalse(snap.keyPackage.stale)
        XCTAssertTrue(snap.keyPackage.isRegistered)

        XCTAssertEqual(snap.cachedKpPubkeys, ["typedcached1", "typedcached2"])
        XCTAssertEqual(snap.invitesChipLabel, "1 invite")
        XCTAssertTrue(snap.isRegistered)
    }

    /// The key-package `has_*` companions all `false` → every optional field nil,
    /// and `invitesChipLabel` (shell-computed from `pendingWelcomes.count`) returns
    /// nil when there are no pending welcomes. Pins the JSON-`null` parity the
    /// `Equatable` compare in `MarmotStore.apply` depends on.
    func testTypedMarmotSnapshotCompanionFalseYieldsNil() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedMarmotSnapshotDecoder.key,
            schemaId: TypedMarmotSnapshotDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedMarmotSnapshotDecoder.fileIdentifier,
            payload: buildMarmotSnapshot(
                groups: [],
                pendingWelcomes: [],
                keyPackage: KeyPackageFixture(
                    published: false, dTag: nil, ageSecs: nil, stale: false,
                    isRegistered: false),
                cachedKpPubkeys: [],
                isRegistered: false))

        let snap = try XCTUnwrap(TypedMarmotSnapshotDecoder.decode(from: [envelope]))
        XCTAssertEqual(snap.groups, [])
        XCTAssertEqual(snap.pendingWelcomes, [])
        XCTAssertNil(snap.keyPackage.dTag)
        XCTAssertNil(snap.keyPackage.ageSecs)
        XCTAssertFalse(snap.keyPackage.isRegistered)
        XCTAssertNil(snap.invitesChipLabel)
        XCTAssertFalse(snap.isRegistered)
    }

    func testWrongSchemaMarmotSnapshotFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedMarmotSnapshotDecoder.key,
            schemaId: "not.marmot.snapshot",
            schemaVersion: 1,
            fileIdentifier: TypedMarmotSnapshotDecoder.fileIdentifier,
            payload: buildMarmotSnapshot(
                groups: [], pendingWelcomes: [],
                keyPackage: KeyPackageFixture.empty,
                cachedKpPubkeys: [], isRegistered: false))
        XCTAssertNil(TypedMarmotSnapshotDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    // MARK: - nmp.marmot.messages (NMMG)

    func testMarmotMessagesSidecarIdentityIsExact() {
        XCTAssertEqual(TypedMarmotMessagesDecoder.key, "nmp.marmot.messages")
        XCTAssertEqual(TypedMarmotMessagesDecoder.schemaId, "nmp.marmot.messages")
        XCTAssertEqual(TypedMarmotMessagesDecoder.fileIdentifier, "NMMG")
    }

    func testTypedMarmotMessagesSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedMarmotMessagesDecoder.key,
            schemaId: TypedMarmotMessagesDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedMarmotMessagesDecoder.fileIdentifier,
            payload: buildMarmotMessages([
                GroupMessagesFixture(groupIdHex: "typedgroupA", messages: [
                    MsgFixture(
                        id: "typedmsg1", senderPubkeyHex: "typedsenderA",
                        content: "typed first", createdAt: 1_700_000_201, epoch: 5),
                    // `has_epoch == false` → epoch must surface nil.
                    MsgFixture(
                        id: "typedmsg2", senderPubkeyHex: "typedsenderB",
                        content: "typed second", createdAt: 1_700_000_202, epoch: nil),
                ]),
                GroupMessagesFixture(groupIdHex: "typedgroupB", messages: []),
            ]))

        let map = try XCTUnwrap(
            TypedMarmotMessagesDecoder.decode(from: [envelope]),
            "well-formed NMMG sidecar must decode")

        XCTAssertEqual(Set(map.keys), ["typedgroupA", "typedgroupB"])
        let groupA = try XCTUnwrap(map["typedgroupA"])
        // Message order within a group is preserved verbatim (Rust owns it).
        XCTAssertEqual(groupA.map(\.id), ["typedmsg1", "typedmsg2"])
        XCTAssertEqual(groupA[0].senderPubkeyHex, "typedsenderA")
        XCTAssertEqual(groupA[0].content, "typed first")
        XCTAssertEqual(groupA[0].createdAt, 1_700_000_201)
        XCTAssertEqual(groupA[0].epoch, 5)
        // `has_epoch == false` → nil (NOT 0), byte-identical to the JSON `null`.
        XCTAssertNil(groupA[1].epoch)

        XCTAssertEqual(map["typedgroupB"], [])
    }

    func testWrongSchemaMarmotMessagesFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedMarmotMessagesDecoder.key,
            schemaId: "not.marmot.messages",
            schemaVersion: 1,
            fileIdentifier: TypedMarmotMessagesDecoder.fileIdentifier,
            payload: buildMarmotMessages([]))
        XCTAssertNil(TypedMarmotMessagesDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyEnvelopesYieldNilForBothMarmotKeys() {
        XCTAssertNil(TypedMarmotSnapshotDecoder.decode(from: []))
        XCTAssertNil(TypedMarmotMessagesDecoder.decode(from: []))
    }

    // MARK: - Fixtures + FlatBuffers builders

    private struct GroupFixture {
        let idHex: String
        let name: String
        let members: [String]
        let memberCount: UInt32
        let unreadCount: UInt32?
        let lastMsgAt: UInt64?
    }

    private struct WelcomeFixture {
        let idHex: String
        let groupName: String
        let inviterNpub: String
    }

    private struct KeyPackageFixture {
        let published: Bool
        let dTag: String?
        let ageSecs: UInt64?
        let stale: Bool
        let isRegistered: Bool

        static let empty = KeyPackageFixture(
            published: false, dTag: nil, ageSecs: nil, stale: false,
            isRegistered: false)
    }

    private func buildMarmotSnapshot(
        groups: [GroupFixture],
        pendingWelcomes: [WelcomeFixture],
        keyPackage: KeyPackageFixture,
        cachedKpPubkeys: [String],
        isRegistered: Bool
    ) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 1024)

        let groupOffsets: [Offset] = groups.map { g in
            let idOff = fbb.create(string: g.idHex)
            let nameOff = fbb.create(string: g.name)
            let membersVec = fbb.createVector(
                ofOffsets: g.members.map { fbb.create(string: $0) })
            return nmp_marmot_MarmotGroupRow.createMarmotGroupRow(
                &fbb,
                idHexOffset: idOff,
                nameOffset: nameOff,
                membersVectorOffset: membersVec,
                memberCount: g.memberCount,
                hasUnreadCount: g.unreadCount != nil,
                unreadCount: g.unreadCount ?? 0,
                hasLastMsgAt: g.lastMsgAt != nil,
                lastMsgAt: g.lastMsgAt ?? 0)
        }
        let groupsVec = fbb.createVector(ofOffsets: groupOffsets)

        let welcomeOffsets: [Offset] = pendingWelcomes.map { w in
            let idOff = fbb.create(string: w.idHex)
            let groupNameOff = fbb.create(string: w.groupName)
            let inviterOff = fbb.create(string: w.inviterNpub)
            return nmp_marmot_PendingWelcomeRow.createPendingWelcomeRow(
                &fbb,
                idHexOffset: idOff,
                groupNameOffset: groupNameOff,
                inviterNpubOffset: inviterOff)
        }
        let welcomesVec = fbb.createVector(ofOffsets: welcomeOffsets)

        let dTagOff = keyPackage.dTag.map { fbb.create(string: $0) } ?? Offset()
        let keyPackageOff = nmp_marmot_KeyPackageStatus.createKeyPackageStatus(
            &fbb,
            published: keyPackage.published,
            hasDTag: keyPackage.dTag != nil,
            dTagOffset: dTagOff,
            hasAgeSecs: keyPackage.ageSecs != nil,
            ageSecs: keyPackage.ageSecs ?? 0,
            stale: keyPackage.stale,
            isRegistered: keyPackage.isRegistered)

        let cachedVec = fbb.createVector(
            ofOffsets: cachedKpPubkeys.map { fbb.create(string: $0) })

        let root = nmp_marmot_MarmotSnapshot.createMarmotSnapshot(
            &fbb,
            groupsVectorOffset: groupsVec,
            pendingWelcomesVectorOffset: welcomesVec,
            keyPackageOffset: keyPackageOff,
            cachedKpPubkeysVectorOffset: cachedVec,
            isRegistered: isRegistered)
        nmp_marmot_MarmotSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private struct MsgFixture {
        let id: String
        let senderPubkeyHex: String
        let content: String
        let createdAt: UInt64
        let epoch: UInt64?
    }

    private struct GroupMessagesFixture {
        let groupIdHex: String
        let messages: [MsgFixture]
    }

    private func buildMarmotMessages(_ groups: [GroupMessagesFixture]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let groupOffsets: [Offset] = groups.map { group in
            let msgOffsets: [Offset] = group.messages.map { msg in
                let idOff = fbb.create(string: msg.id)
                let senderOff = fbb.create(string: msg.senderPubkeyHex)
                let contentOff = fbb.create(string: msg.content)
                return nmp_marmot_MarmotMessageRow.createMarmotMessageRow(
                    &fbb,
                    idOffset: idOff,
                    senderPubkeyHexOffset: senderOff,
                    contentOffset: contentOff,
                    createdAt: msg.createdAt,
                    hasEpoch: msg.epoch != nil,
                    epoch: msg.epoch ?? 0)
            }
            let groupIdOff = fbb.create(string: group.groupIdHex)
            let msgsVec = fbb.createVector(ofOffsets: msgOffsets)
            return nmp_marmot_MarmotGroupMessages.createMarmotGroupMessages(
                &fbb, groupIdHexOffset: groupIdOff, messagesVectorOffset: msgsVec)
        }
        let groupsVec = fbb.createVector(ofOffsets: groupOffsets)
        let root = nmp_marmot_MarmotMessages.createMarmotMessages(
            &fbb, groupsVectorOffset: groupsVec)
        nmp_marmot_MarmotMessages.finish(&fbb, end: root)
        return fbb.data
    }
}
