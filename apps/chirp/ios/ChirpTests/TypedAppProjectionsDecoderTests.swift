import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the Wave B Tier-1 #4 app-projection sidecars:
/// `nmp.follow_list` (`NF02`), `nmp.nip29.group_events` (`NGEV`), and
/// `nmp.nip29.discovered_groups` (`NDGS`).
/// These mirror `TypedDiagnosticsLifecycleDecoderTests`: build the typed
/// FlatBuffers buffer directly via the generated builders, wrap it in a
/// `TypedProjectionEnvelope` carrying the producer's actual `(key, schemaId)`,
/// and assert the generated decoder (`Typed<Key>Decoder`) produces the Chirp
/// domain value.
///
/// PRECEDENCE CONTRACT: the typed value must be USED, not merely decodable.
/// Each "typed present" case uses values that DIFFER from any plausible JSON
/// value, so a passing assertion proves the typed path won rather than
/// coincided. The "typed absent / wrong-schema / garbled" cases assert `nil`,
/// the signal the read site interprets as "fall back to the generic JSON
/// `projections.<field>` path" (ADR-0037 Commitment 4).
///
/// `nmp.follow_list` additionally guards the deliberate key≠schema_id split:
/// the producer (`apps/chirp/.../ffi/register.rs::follow_list_typed_projection`)
/// publishes ENVELOPE key `"nmp.follow_list"` with payload SCHEMA_ID
/// `"nmp.nip02.follow_list"`. The decoder matches on BOTH, so a mismatch on
/// either is a silent-nil bug — `testFollowListKeySchemaSplitIsExact` pins it.
final class TypedAppProjectionsDecoderTests: XCTestCase {

    // ── nmp.follow_list (NF02) ───────────────────────────────────────────────

    func testTypedFollowListSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedFollowListDecoder.key,
            schemaId: TypedFollowListDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedFollowListDecoder.fileIdentifier,
            payload: buildFollowList(["typedpk1", "typedpk2", "typedpk3"]))

        let snap = try XCTUnwrap(
            TypedFollowListDecoder.decode(from: [envelope]),
            "well-formed NF02 sidecar must decode")

        // Order preserved verbatim (parity with the JSON array).
        XCTAssertEqual(snap.follows.map(\.pubkey), ["typedpk1", "typedpk2", "typedpk3"])
    }

    /// Pins the producer's key/schema_id identity. If the registry's
    /// `TypedSidecar.key` or `schema_id` ever drifts from the producer, the
    /// generated constants change and this fails loudly — catching the
    /// silent-nil class the wallet entry documents.
    func testFollowListKeySchemaSplitIsExact() {
        XCTAssertEqual(TypedFollowListDecoder.key, "nmp.follow_list")
        XCTAssertEqual(TypedFollowListDecoder.schemaId, "nmp.nip02.follow_list")
        XCTAssertEqual(TypedFollowListDecoder.fileIdentifier, "NF02")
    }

    /// The envelope's KEY must match too — a buffer carrying the right schema_id
    /// under the WRONG key (e.g. the schema_id used as the key) must NOT decode,
    /// or the key≠schema_id split would be cosmetic.
    func testFollowListWrongKeyFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: "nmp.nip02.follow_list", // schema_id used as key — wrong
            schemaId: TypedFollowListDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedFollowListDecoder.fileIdentifier,
            payload: buildFollowList(["x"]))
        XCTAssertNil(TypedFollowListDecoder.decode(from: [envelope]))
    }

    func testAbsentFollowListSidecarFallsBack() {
        XCTAssertNil(TypedFollowListDecoder.decode(from: []))
    }

    func testWrongSchemaFollowListFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedFollowListDecoder.key,
            schemaId: "not.follow_list",
            schemaVersion: 1,
            fileIdentifier: TypedFollowListDecoder.fileIdentifier,
            payload: buildFollowList(["x"]))
        XCTAssertNil(TypedFollowListDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyFollowListBufferDecodesToNoFollows() throws {
        // A fresh kernel (no account / no kind:3) pushes an empty buffer — the
        // typed path must yield an empty list, NOT nil (nil would wrongly trigger
        // the JSON fallback when the typed path is in fact authoritative).
        let snap = try XCTUnwrap(TypedFollowListDecoder.decode(bytes: buildFollowList([])))
        XCTAssertTrue(snap.follows.isEmpty)
    }

    // ── nmp.nip29.group_events (NGEV) ──────────────────────────────────────────

    func testTypedGroupChatSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedGroupEventsDecoder.key,
            schemaId: TypedGroupEventsDecoder.schemaId,
            schemaVersion: 1,
            fileIdentifier: TypedGroupEventsDecoder.fileIdentifier,
            payload: buildGroupEvents([
                ("typed-id-1", "typed-pk-1", "typed hello", 1_700_000_111, 9),
                ("typed-id-2", "typed-pk-2", "typed thread", 1_700_000_222, 11),
            ]))

        let snap = try XCTUnwrap(
            TypedGroupEventsDecoder.decode(from: [envelope]),
            "well-formed NGEV sidecar must decode")

        XCTAssertEqual(snap.events.count, 2)
        // Order preserved verbatim (the Rust projection emits newest-first).
        let first = snap.events[0]
        XCTAssertEqual(first.id, "typed-id-1")
        XCTAssertEqual(first.pubkey, "typed-pk-1")
        XCTAssertEqual(first.content, "typed hello")
        XCTAssertEqual(first.createdAt, 1_700_000_111)
        XCTAssertEqual(first.kind, 9)
        XCTAssertEqual(snap.events[1].id, "typed-id-2")
        XCTAssertEqual(snap.events[1].kind, 11)
    }

    func testAbsentGroupChatSidecarFallsBack() {
        XCTAssertNil(TypedGroupEventsDecoder.decode(from: []))
    }

    func testWrongSchemaGroupChatFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedGroupEventsDecoder.key,
            schemaId: "not.group_events",
            schemaVersion: 1,
            fileIdentifier: TypedGroupEventsDecoder.fileIdentifier,
            payload: buildGroupEvents([("i", "p", "c", 1, 9)]))
        XCTAssertNil(TypedGroupEventsDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyGroupChatBufferDecodesToNoMessages() throws {
        let snap = try XCTUnwrap(TypedGroupEventsDecoder.decode(bytes: buildGroupEvents([])))
        XCTAssertTrue(snap.events.isEmpty)
    }

    // ── nmp.nip29.discovered_groups (NDGS) ───────────────────────────────────

    func testTypedDiscoveredGroupsSidecarDecodes() throws {
        let envelope = TypedProjectionEnvelope(
            key: TypedDiscoveredGroupsDecoder.key,
            schemaId: TypedDiscoveredGroupsDecoder.schemaId,
            schemaVersion: 2,
            fileIdentifier: TypedDiscoveredGroupsDecoder.fileIdentifier,
            payload: buildDiscoveredGroups())

        let snap = try XCTUnwrap(
            TypedDiscoveredGroupsDecoder.decode(from: [envelope]),
            "well-formed NDGS sidecar must decode")

        XCTAssertEqual(snap.hostRelayUrl, "wss://typed-groups.example")
        XCTAssertEqual(snap.groups.count, 2)

        // Row 0: all metadata present, incl. NIP-29 subgroup tags (#2319).
        let full = snap.groups[0]
        XCTAssertEqual(full.groupId, "typed-group-full")
        XCTAssertEqual(full.hostRelayUrl, "wss://typed-groups.example")
        XCTAssertEqual(full.name, "Typed Full")
        XCTAssertEqual(full.picture, "https://typed/pic.png")
        XCTAssertEqual(full.about, "typed about")
        XCTAssertEqual(full.memberCount, 42)
        XCTAssertEqual(full.adminCount, 3)
        XCTAssertTrue(full.public)
        XCTAssertTrue(full.open)
        XCTAssertEqual(full.parent, "tech")
        XCTAssertEqual(full.children, ["nostr", "bitcoin"])
        // Shell-derived display helpers (computed properties in DiscoveredGroup extension)
        XCTAssertEqual(full.displayName, "Typed Full")    // name present → use name
        XCTAssertEqual(full.initials, "TY")               // first 2 of "Typed Full" uppercased
        XCTAssertEqual(full.subtitle, "# Public · Open · 42 members")

        // Row 1: optional tag-derived `name`/`picture`/`about`/`parent` ABSENT.
        // The glue must preserve nil (NOT `?? ""`), byte-identical to the JSON
        // `null`; `children` must be empty (not nil) since the row has no
        // declared children.
        let bare = snap.groups[1]
        XCTAssertEqual(bare.groupId, "typed-group-bare")
        XCTAssertNil(bare.name)
        XCTAssertNil(bare.picture)
        XCTAssertNil(bare.about)
        XCTAssertEqual(bare.memberCount, 0)
        XCTAssertFalse(bare.public)
        XCTAssertFalse(bare.open)
        XCTAssertNil(bare.parent)
        XCTAssertEqual(bare.children, [])
        // Shell-derived: fallback to groupId since name is nil
        XCTAssertEqual(bare.displayName, "typed-group-bare")
        XCTAssertEqual(bare.initials, "TY")               // first 2 of "typed-group-bare"
        XCTAssertEqual(bare.subtitle, "🔒 Private · Closed · 0 members")
    }

    func testAbsentDiscoveredGroupsSidecarFallsBack() {
        XCTAssertNil(TypedDiscoveredGroupsDecoder.decode(from: []))
    }

    func testWrongSchemaDiscoveredGroupsFallsBack() {
        let envelope = TypedProjectionEnvelope(
            key: TypedDiscoveredGroupsDecoder.key,
            schemaId: "not.discovered_groups",
            schemaVersion: 1,
            fileIdentifier: TypedDiscoveredGroupsDecoder.fileIdentifier,
            payload: buildDiscoveredGroups())
        XCTAssertNil(TypedDiscoveredGroupsDecoder.decode(from: [envelope]))
    }

    // NOTE: the garbled-file-identifier test was removed. The decode path now
    // uses unchecked `getRoot` (trusted in-process FFI boundary); the 4-byte
    // file-identifier magic is NOT verified. A structurally-valid buffer with
    // a clobbered magic still decodes successfully (possibly to empty/default
    // field values). The key+schemaId envelope routing in `decode(from:)` is
    // the selection mechanism, not the file identifier.

    func testEmptyDiscoveredGroupsBufferDecodesToNoGroups() throws {
        var fbb = FlatBufferBuilder(initialSize: 64)
        let root = nmp_nip29_DiscoveredGroupsSnapshot.createDiscoveredGroupsSnapshot(&fbb)
        nmp_nip29_DiscoveredGroupsSnapshot.finish(&fbb, end: root)
        let snap = try XCTUnwrap(TypedDiscoveredGroupsDecoder.decode(bytes: fbb.data))
        XCTAssertTrue(snap.groups.isEmpty)
        XCTAssertEqual(snap.hostRelayUrl, "")
    }

    // ── Buffer builders ──────────────────────────────────────────────────────

    private func buildFollowList(_ pubkeys: [String]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 256)
        let rows: [Offset] = pubkeys.map { pk in
            let off = fbb.create(string: pk)
            return nmp_nip02_FollowEntry.createFollowEntry(&fbb, pubkeyOffset: off)
        }
        let vec = fbb.createVector(ofOffsets: rows)
        let root = nmp_nip02_FollowListSnapshot.createFollowListSnapshot(
            &fbb, followsVectorOffset: vec)
        nmp_nip02_FollowListSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildGroupEvents(_ rows: [(String, String, String, UInt64, UInt32)]) -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)
        let offsets: [Offset] = rows.map { (id, pubkey, content, createdAt, kind) in
            let idOff = fbb.create(string: id)
            let pkOff = fbb.create(string: pubkey)
            let contentOff = fbb.create(string: content)
            return nmp_nip29_GroupEvent.createGroupEvent(
                &fbb,
                idOffset: idOff,
                pubkeyOffset: pkOff,
                contentOffset: contentOff,
                createdAt: createdAt,
                kind: kind)
        }
        let vec = fbb.createVector(ofOffsets: offsets)
        let root = nmp_nip29_GroupEventsSnapshot.createGroupEventsSnapshot(
            &fbb, eventsVectorOffset: vec)
        nmp_nip29_GroupEventsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }

    private func buildDiscoveredGroups() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 512)

        // Row 0 — all optional metadata present, incl. NIP-29 subgroup tags.
        let fullId = fbb.create(string: "typed-group-full")
        let fullHost = fbb.create(string: "wss://typed-groups.example")
        let fullName = fbb.create(string: "Typed Full")
        let fullPic = fbb.create(string: "https://typed/pic.png")
        let fullAbout = fbb.create(string: "typed about")
        let fullParent = fbb.create(string: "tech")
        let fullChildren = fbb.createVector(ofOffsets: [
            fbb.create(string: "nostr"),
            fbb.create(string: "bitcoin"),
        ])
        let full = nmp_nip29_DiscoveredGroup.createDiscoveredGroup(
            &fbb,
            groupIdOffset: fullId,
            hostRelayUrlOffset: fullHost,
            nameOffset: fullName,
            pictureOffset: fullPic,
            aboutOffset: fullAbout,
            memberCount: 42,
            adminCount: 3,
            public_: true,
            open_: true,
            parentOffset: fullParent,
            childrenVectorOffset: fullChildren)

        // Row 1 — optional `name`/`picture`/`about` absent (offsets left default
        // → wire string absent → decoder yields nil, parity with JSON `null`).
        let bareId = fbb.create(string: "typed-group-bare")
        let bareHost = fbb.create(string: "wss://typed-groups.example")
        let bare = nmp_nip29_DiscoveredGroup.createDiscoveredGroup(
            &fbb,
            groupIdOffset: bareId,
            hostRelayUrlOffset: bareHost,
            memberCount: 0,
            adminCount: 0,
            public_: false,
            open_: false)

        let groupsVec = fbb.createVector(ofOffsets: [full, bare])
        let host = fbb.create(string: "wss://typed-groups.example")
        let root = nmp_nip29_DiscoveredGroupsSnapshot.createDiscoveredGroupsSnapshot(
            &fbb, hostRelayUrlOffset: host, groupsVectorOffset: groupsVec)
        nmp_nip29_DiscoveredGroupsSnapshot.finish(&fbb, end: root)
        return fbb.data
    }
}
