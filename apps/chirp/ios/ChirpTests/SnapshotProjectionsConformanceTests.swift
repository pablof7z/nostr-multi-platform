import XCTest
@testable import Chirp

/// Conformance test — every Rust-registered snapshot-projection key that
/// `SnapshotProjections.CodingKeys` claims to map must actually decode
/// non-nil from a synthetic kernel snapshot containing that key.
///
/// ## Why this exists
///
/// `SnapshotProjections` declares an explicit `CodingKeys` enum (see
/// `KernelBridge.swift`). Declaring `CodingKeys` overrides Swift's synthesised
/// decoder entirely: any case whose raw value does NOT match the kernel's
/// post-`.convertFromSnakeCase` JSON key would silently decode the field to
/// `nil` with NO compiler warning, NO runtime error, and NO obvious failure —
/// the data simply disappears.
///
/// This pattern has bitten us at least twice. The decoder uses
/// `.convertFromSnakeCase`, which splits on `_` only (`.` is opaque), so the
/// kernel's dotted keys (e.g. `"nmp.nip29.group_events"`) transform to
/// `"nmp.nip29.groupEvents"` — and the `CodingKeys` raw value MUST be that
/// post-transform string, not the bare camelCase name the synthesised default
/// would have produced. A subtle drift between the kernel key, the
/// `.convertFromSnakeCase` transform, and the `CodingKeys` raw value is
/// invisible at compile time.
///
/// ## Contract
///
/// For every `CodingKeys` case that maps to a Rust-registered snapshot
/// projection (i.e. every dotted-key case + every domain projection the iOS
/// shell binds against), the synthetic JSON below MUST contain a payload
/// under the kernel-emitted key, and the corresponding `SnapshotProjections`
/// property MUST decode non-nil. If a new projection is wired in Rust under
/// a new key, add:
///
///   1. The field to `SnapshotProjections`,
///   2. The case to `SnapshotProjections.CodingKeys` (with the
///      post-`.convertFromSnakeCase` raw value),
///   3. A row to `testSnapshotProjectionsCoverAllRegisteredKeys` below.
///
/// Failure here means a snapshot projection silently decodes to `nil`. Do NOT
/// "fix" it by deleting the `CodingKeys` enum (synthesised decoding would
/// regress the dotted-key fields). Fix the case's raw value instead.
///
/// ## Known gap
///
/// None for the JSON `Decodable` path — every registered Rust projection that
/// ships as a JSON `SnapshotProjections` field (`nmp.nip29.group_events`,
/// `nmp.nip29.discovered_groups`, `nmp.nip29.group_defaults`,
/// `nmp.nip17.dm_inbox`, `nmp.follow_list`, `nmp.nip17.dm_relay_list`,
/// `nmp.marmot.snapshot`, `nmp.marmot.messages`) has its decoder covered by
/// this conformance test
/// (V-107 / ADR-0039).
///
/// One registered dotted key is DELIBERATELY out of scope here: `nmp.feed.home`
/// is a typed FlatBuffers NOFS sidecar (`schema_id "nmp.nip01.opfeed"`,
/// `swift_reader_type: None`), decoded by the hand-written `TypedHomeFeedDecoder`
/// — NOT a JSON `SnapshotProjections` field — so it has no `XCTAssertNotNil`
/// here. It IS tracked by the Rust `all_dotted_keys_are_present` registry test
/// (which asserts all nine dotted keys, this one included).
final class SnapshotProjectionsConformanceTests: XCTestCase {

    /// The exact decoder configuration `KernelHandle.decode` uses for the
    /// kernel snapshot inner payload. Test reproductions of decode bugs
    /// require bit-for-bit reproduction of this configuration.
    private func snapshotDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    /// Synthetic JSON containing every Rust-registered projection key that
    /// has a corresponding `SnapshotProjections` field on `master`. Decodes
    /// the whole `SnapshotProjections` struct, then asserts each field
    /// non-nil — the test fails LOUDLY if any case-to-key mapping drifts.
    ///
    /// The JSON keys are written in their kernel-emitted form (snake_case +
    /// dotted), since `.convertFromSnakeCase` runs on the way IN to the
    /// decoder. The inner struct shapes match the non-optional fields each
    /// `Decodable` mirror declares in `KernelBridge.swift` — if any of those
    /// shapes drift (e.g. a new required field) this test must be updated
    /// or the whole `SnapshotProjections` decode throws.
    func testSnapshotProjectionsCoverAllRegisteredKeys() throws {
        let json = """
        {
          "nmp.nip29.group_events": {
            "events": [],
            "group_initials": "?"
          },
          "nmp.nip29.discovered_groups": {
            "host_relay_url": "wss://groups.example.com",
            "groups": []
          },
          "nmp.nip29.group_defaults": {
            "suggested_relay_url": "wss://group-default.example"
          },
          "nmp.nip17.dm_inbox": {
            "conversations": []
          },
          "nmp.follow_list": {
            "follows": []
          },
          "nmp.nip17.dm_relay_list": {
            "active_pubkey": null,
            "read_relay_urls": []
          },
          "nmp.marmot.snapshot": {
            "groups": [
              {
                "id_hex": "aabbccdd",
                "name": "Test Group",
                "display_name": "Test Group",
                "initials": "TG",
                "members": ["pk1", "pk2"],
                "member_count": 2,
                "unread_count": 3,
                "last_msg_at": 1700000000
              }
            ],
            "pending_welcomes": [],
            "key_package": {
              "published": false,
              "stale": false,
              "is_registered": false
            },
            "cached_kp_pubkeys": [],
            "is_registered": false
          },
          "nmp.marmot.messages": {
            "aabbccdd": [
              {
                "id": "msg1",
                "sender_pubkey_hex": "deadbeef",
                "content": "hello from the projection",
                "created_at": 1700000001,
                "epoch": 7
              }
            ]
          }
        }
        """
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data(json.utf8))

        // Every field below maps to a Rust-registered key. A nil here means
        // the `CodingKeys` case's raw value drifted from the kernel-emitted
        // key (after `.convertFromSnakeCase`). The message names the exact
        // case to inspect so the failure is self-diagnosing.
        XCTAssertNotNil(
            projections.groupEvents,
            "SnapshotProjections.groupEvents decoded nil — check CodingKeys.groupEvents raw value matches \"nmp.nip29.groupEvents\" (post-convertFromSnakeCase of \"nmp.nip29.group_events\")")
        XCTAssertNotNil(
            projections.discoveredGroups,
            "SnapshotProjections.discoveredGroups decoded nil — check CodingKeys.discoveredGroups raw value matches \"nmp.nip29.discoveredGroups\" (post-convertFromSnakeCase of \"nmp.nip29.discovered_groups\")")
        XCTAssertNotNil(
            projections.groupDefaults,
            "SnapshotProjections.groupDefaults decoded nil — check CodingKeys.groupDefaults raw value matches \"nmp.nip29.groupDefaults\" (post-convertFromSnakeCase of \"nmp.nip29.group_defaults\")")
        XCTAssertNotNil(
            projections.dmInbox,
            "SnapshotProjections.dmInbox decoded nil — check CodingKeys.dmInbox raw value matches \"nmp.nip17.dmInbox\" (post-convertFromSnakeCase of \"nmp.nip17.dm_inbox\")")
        XCTAssertNotNil(
            projections.followList,
            "SnapshotProjections.followList decoded nil — check CodingKeys.followList raw value matches \"nmp.followList\" (post-convertFromSnakeCase of \"nmp.follow_list\")")
        XCTAssertNotNil(
            projections.dmRelayList,
            "SnapshotProjections.dmRelayList decoded nil — check CodingKeys.dmRelayList raw value matches \"nmp.nip17.dmRelayList\" (post-convertFromSnakeCase of \"nmp.nip17.dm_relay_list\")")
        // ADR-0063 Lane H: claimedProfiles (KCPR) deleted from SnapshotProjections.
        // V-107 / ADR-0039: Marmot push projections.
        XCTAssertNotNil(
            projections.marmotSnapshot,
            "SnapshotProjections.marmotSnapshot decoded nil — check CodingKeys.marmotSnapshot raw value matches \"nmp.marmot.snapshot\" (no underscore, no transform needed)")
        XCTAssertNotNil(
            projections.marmotMessages,
            "SnapshotProjections.marmotMessages decoded nil — check CodingKeys.marmotMessages raw value matches \"nmp.marmot.messages\" (no underscore, no transform needed)")

        // V-107 deletion-safety: prove the POPULATED marmot rows decode field-by-field
        // through .convertFromSnakeCase. An empty-map decode succeeds trivially and
        // would NOT catch a snake_case→camelCase CodingKey drift (the keyNotFound class
        // that silently blanks rows). These assertions are the runtime proof that the
        // pull-symbol fallback was safe to delete.
        let group = projections.marmotSnapshot?.groups.first
        XCTAssertEqual(group?.idHex, "aabbccdd",
            "MarmotGroup.idHex must decode from Rust \"id_hex\" via convertFromSnakeCase")
        XCTAssertEqual(group?.memberCount, 2,
            "MarmotGroup.memberCount must decode from Rust \"member_count\"")
        XCTAssertEqual(group?.unreadCount, 3,
            "MarmotGroup.unreadCount must decode from Rust \"unread_count\"")
        XCTAssertEqual(group?.lastMsgAt, 1_700_000_000,
            "MarmotGroup.lastMsgAt must decode from Rust \"last_msg_at\"")

        let msg = projections.marmotMessages?["aabbccdd"]?.first
        XCTAssertEqual(msg?.senderPubkeyHex, "deadbeef",
            "MarmotMessage.senderPubkeyHex must decode from Rust \"sender_pubkey_hex\"")
        XCTAssertEqual(msg?.content, "hello from the projection")
        XCTAssertEqual(msg?.createdAt, 1_700_000_001,
            "MarmotMessage.createdAt must decode from Rust \"created_at\"")
        XCTAssertEqual(msg?.epoch, 7,
            "MarmotMessage.epoch must decode from Rust \"epoch\"")
    }

    /// Sanity check: all snapshot projection fields default to nil when the
    /// kernel emits an empty projections map (an older kernel build that
    /// predates the projections, or a fresh actor with no registrations yet).
    /// This is the steady-state any new field MUST tolerate (D1 — never
    /// fail a decode on a missing projection).
    func testEmptyProjectionsMapDecodesWithAllNils() throws {
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data("{}".utf8))
        XCTAssertNil(projections.groupEvents)
        XCTAssertNil(projections.discoveredGroups)
        XCTAssertNil(projections.groupDefaults)
        XCTAssertNil(projections.dmInbox)
        XCTAssertNil(projections.followList)
        XCTAssertNil(projections.dmRelayList)
        // ADR-0063 Lane H: claimedProfiles (KCPR) deleted from SnapshotProjections.
        // V-107 / ADR-0039: Marmot push projections also nil on empty map.
        XCTAssertNil(projections.marmotSnapshot)
        XCTAssertNil(projections.marmotMessages)
    }
}
