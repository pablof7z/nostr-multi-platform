import XCTest
@testable import Chirp

/// Pure JSON-decode tests for the NIP-29 group-chat read model.
///
/// These need no kernel, no FFI, and no simulator wiring — they validate
/// the one subtle thing that the type checker cannot: that the dotted
/// projection key `"nmp.nip29.group_chat"` survives the `JSONDecoder`'s
/// `.convertFromSnakeCase` strategy and lands on `SnapshotProjections.groupChat`.
///
/// `.convertFromSnakeCase` transforms each JSON key BEFORE matching it
/// against a `CodingKey.stringValue`. It splits on `_` only, so
/// `"nmp.nip29.group_chat"` → `"nmp.nip29.groupChat"` — which is exactly the raw
/// value `SnapshotProjections.CodingKeys.groupChat` declares. If that ever
/// drifts, `SnapshotProjections` would silently decode `groupChat` as `nil`
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

    /// `"nmp.nip29.group_chat"` decodes onto `SnapshotProjections.groupChat`
    /// despite the dotted key + `.convertFromSnakeCase`.
    ///
    /// LOAD-BEARING: if `SnapshotProjections` ever throws on this payload,
    /// `KernelHandle.decode` returns `nil` and the ENTIRE kernel snapshot is
    /// discarded — not just the group-chat field. Do NOT "fix" a failure
    /// here by deleting the explicit `CodingKeys` enum on `SnapshotProjections`;
    /// that enum exists precisely so the dotted key maps correctly. If the
    /// kernel renames the key, update `CodingKeys.groupChat`'s raw value to
    /// the post-`.convertFromSnakeCase` form of the new key.
    func testGroupChatProjectionKeyDecodes() throws {
        // `created_at_display` is the V-22 thin-shell field — the Rust
        // projection (re)computes it on every snapshot tick via
        // `nmp_nip29::projection::group_chat::format_ago_secs` so the host
        // view binds it directly and never reaches for
        // `RelativeDateTimeFormatter`.
        //
        // `author_display` / `author_initials` / `author_color_hex` are the
        // V-25 thin-shell fields — pure functions of the event author,
        // computed in `nmp_nip29::projection::group_chat` at ingest. The
        // view binds them directly and never slices the hex string itself.
        let json = """
        {
          "nmp.nip29.group_chat": {
            "messages": [
              { "id": "e1", "pubkey": "ab12", "content": "hello",
                "created_at": 200, "created_at_display": "5s ago",
                "author_display": "ab12", "author_initials": "AB",
                "author_color_hex": "93E7AB", "kind": 9 },
              { "id": "e0", "pubkey": "cd34", "content": "earlier",
                "created_at": 100, "created_at_display": "2m ago",
                "author_display": "cd34", "author_initials": "CD",
                "author_color_hex": "950933", "kind": 11 }
            ]
          }
        }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))

        let chat = try XCTUnwrap(projections.groupChat,
            "nmp.nip29.group_chat must decode onto SnapshotProjections.groupChat")
        XCTAssertEqual(chat.messages.count, 2)
        // Order is preserved verbatim from the JSON — the Rust projection
        // already emits newest-first; Swift does not re-sort.
        XCTAssertEqual(chat.messages[0].id, "e1")
        XCTAssertEqual(chat.messages[0].pubkey, "ab12")
        XCTAssertEqual(chat.messages[0].content, "hello")
        XCTAssertEqual(chat.messages[0].createdAt, 200)
        XCTAssertEqual(chat.messages[0].createdAtDisplay, "5s ago")
        XCTAssertEqual(chat.messages[0].kind, 9)
        // V-25: the three author display fields land verbatim on the
        // camelCase Swift properties via `.convertFromSnakeCase`.
        XCTAssertEqual(chat.messages[0].authorDisplay, "ab12")
        XCTAssertEqual(chat.messages[0].authorInitials, "AB")
        XCTAssertEqual(chat.messages[0].authorColorHex, "93E7AB")
        XCTAssertEqual(chat.messages[1].createdAtDisplay, "2m ago")
        XCTAssertEqual(chat.messages[1].kind, 11)
        XCTAssertEqual(chat.messages[1].authorDisplay, "cd34")
        XCTAssertEqual(chat.messages[1].authorInitials, "CD")
        XCTAssertEqual(chat.messages[1].authorColorHex, "950933")
    }

    /// A snapshot with no `nip29.group_chat` key leaves `groupChat` nil and
    /// still decodes the rest of the projections map — i.e. the new
    /// optional field is non-breaking for an older / un-wired kernel.
    func testGroupChatAbsentLeavesNilWithoutBreakingDecode() throws {
        let json = """
        { "active_account": "npub1xyz" }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))
        XCTAssertNil(projections.groupChat)
        XCTAssertEqual(projections.activeAccount, "npub1xyz")
    }

    /// A registered-but-empty projection decodes to an empty message list,
    /// not nil — the state a freshly-wired group reports before any event.
    func testEmptyGroupChatProjectionDecodes() throws {
        let json = """
        { "nmp.nip29.group_chat": { "messages": [] } }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))
        XCTAssertEqual(projections.groupChat, GroupChatSnapshot.empty)
    }

    /// `GroupId.jsonObject` produces the snake_case shape the Rust
    /// `nmp_nip29::GroupId` deserializes from — the FFI contract for both
    /// `nmp_app_chirp_register_group_chat` and the `nmp.nip29.post_chat_message`
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
