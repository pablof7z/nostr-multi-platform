import XCTest
@testable import Chirp

/// Pure JSON-decode tests for the NIP-29 group-chat read model.
///
/// These need no kernel, no FFI, and no simulator wiring — they validate
/// the one subtle thing that the type checker cannot: that the dotted
/// projection key `"nmp.nip29.group_events"` survives the `JSONDecoder`'s
/// `.convertFromSnakeCase` strategy and lands on `SnapshotProjections.groupEvents`.
///
/// `.convertFromSnakeCase` transforms each JSON key BEFORE matching it
/// against a `CodingKey.stringValue`. It splits on `_` only, so
/// `"nmp.nip29.group_events"` → `"nmp.nip29.groupEvents"` — which is exactly the raw
/// value `SnapshotProjections.CodingKeys.groupEvents` declares. If that ever
/// drifts, `SnapshotProjections` would silently decode `groupEvents` as `nil`
/// (or, worse, drop the whole snapshot — see `KernelHandle.decode`), so it
/// is worth a regression test.
final class GroupChatDecodeTests: XCTestCase {

    /// The exact decoder configuration `KernelHandle.decode` uses for the
    /// kernel snapshot inner payload.
    private func snapshotDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    /// `"nmp.nip29.group_events"` decodes onto `SnapshotProjections.groupEvents`
    /// despite the dotted key + `.convertFromSnakeCase`.
    ///
    /// LOAD-BEARING: if `SnapshotProjections` ever throws on this payload,
    /// `KernelHandle.decode` returns `nil` and the ENTIRE kernel snapshot is
    /// discarded — not just the group-chat field. Do NOT "fix" a failure
    /// here by deleting the explicit `CodingKeys` enum on `SnapshotProjections`;
    /// that enum exists precisely so the dotted key maps correctly. If the
    /// kernel renames the key, update `CodingKeys.groupEvents`'s raw value to
    /// the post-`.convertFromSnakeCase` form of the new key.
    func testGroupChatProjectionKeyDecodes() throws {
        // ADR-0032: the Rust `GroupEvent` projection now carries only
        // raw protocol data — `id`, `pubkey` (hex), `content`, `created_at`
        // (Unix seconds), and `kind`. Display strings (relative-time labels,
        // abbreviated pubkeys, avatar initials / tints) are derived by the
        // presentation layer (`PubkeyFormatting.swift`).
        let json = """
        {
          "nmp.nip29.group_events": {
            "events": [
              { "id": "e1", "pubkey": "ab12", "content": "hello",
                "created_at": 200, "kind": 9 },
              { "id": "e0", "pubkey": "cd34", "content": "earlier",
                "created_at": 100, "kind": 11 }
            ]
          }
        }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))

        let chat = try XCTUnwrap(projections.groupEvents,
            "nmp.nip29.group_events must decode onto SnapshotProjections.groupEvents")
        XCTAssertEqual(chat.events.count, 2)
        // Order is preserved verbatim from the JSON — the Rust projection
        // already emits newest-first; Swift does not re-sort.
        XCTAssertEqual(chat.events[0].id, "e1")
        XCTAssertEqual(chat.events[0].pubkey, "ab12")
        XCTAssertEqual(chat.events[0].content, "hello")
        XCTAssertEqual(chat.events[0].createdAt, 200)
        XCTAssertEqual(chat.events[0].kind, 9)
        XCTAssertEqual(chat.events[1].createdAt, 100)
        XCTAssertEqual(chat.events[1].kind, 11)
        XCTAssertEqual(chat.events[1].pubkey, "cd34")
    }

    /// A snapshot with no `nip29.group_events` key leaves `groupEvents` nil and
    /// still decodes the rest of the projections map — i.e. the new
    /// optional field is non-breaking for an older / un-wired kernel.
    func testGroupChatAbsentLeavesNilWithoutBreakingDecode() throws {
        let json = """
        { "active_account": "npub1xyz" }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))
        XCTAssertNil(projections.groupEvents)
        XCTAssertEqual(projections.activeAccount, "npub1xyz")
    }

    /// A registered-but-empty projection decodes to an empty message list,
    /// not nil — the state a freshly-wired group reports before any event.
    /// ADR-0032: `group_initials` is no longer emitted by Rust (the
    /// presentation layer derives the avatar tile label from
    /// `GroupId.localId`).
    func testEmptyGroupChatProjectionDecodes() throws {
        let json = """
        { "nmp.nip29.group_events": { "events": [] } }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))
        XCTAssertEqual(projections.groupEvents, GroupEventsSnapshot.empty)
    }

    /// `GroupId.jsonObject` produces the snake_case shape the Rust
    /// `nmp_nip29::GroupId` deserializes from — the FFI contract for both
    /// `nmp_app_chirp_register_group_events` and the `nmp.nip29.publish_group_event`
    /// action payload.
    func testGroupIdMarshalsToSnakeCaseJSON() {
        let group = GroupId(
            hostRelayUrl: "wss://groups.example.com", localId: "room-1")
        XCTAssertEqual(group.jsonObject, [
            "host_relay_url": "wss://groups.example.com",
            "local_id": "room-1",
        ])
    }
}
