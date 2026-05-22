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
/// kernel's dotted keys (e.g. `"nmp.nip29.group_chat"`) transform to
/// `"nmp.nip29.groupChat"` — and the `CodingKeys` raw value MUST be that
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
/// ## Known gap (May 2026)
///
/// Two keys are currently registered in Rust but have no Swift field on
/// `master`:
///
///   - `"nmp.nip57.zaps"` (registered in `nmp_app_chirp_register`, the
///     `ZapsAggregateProjection` indexes kind:9735 receipts)
///   - `"nmp.nip17.dm_relay_list"` (registered in `register_dm_runtime`,
///     mirrors the active account's kind:10050 list)
///
/// These are not silent-nil bugs (the Swift fields don't exist at all), but
/// data is flowing unread. When those fields are added to
/// `SnapshotProjections`, extend this test to cover them.
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
          "nmp.nip29.group_chat": {
            "messages": []
          },
          "nmp.nip29.discovered_groups": {
            "host_relay_url": "wss://groups.example.com",
            "groups": []
          },
          "nmp.nip17.dm_inbox": {
            "conversations": []
          },
          "chirp.follow_list": {
            "follows": []
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
            projections.groupChat,
            "SnapshotProjections.groupChat decoded nil — check CodingKeys.groupChat raw value matches \"nmp.nip29.groupChat\" (post-convertFromSnakeCase of \"nmp.nip29.group_chat\")")
        XCTAssertNotNil(
            projections.discoveredGroups,
            "SnapshotProjections.discoveredGroups decoded nil — check CodingKeys.discoveredGroups raw value matches \"nmp.nip29.discoveredGroups\" (post-convertFromSnakeCase of \"nmp.nip29.discovered_groups\")")
        XCTAssertNotNil(
            projections.dmInbox,
            "SnapshotProjections.dmInbox decoded nil — check CodingKeys.dmInbox raw value matches \"nmp.nip17.dmInbox\" (post-convertFromSnakeCase of \"nmp.nip17.dm_inbox\")")
        XCTAssertNotNil(
            projections.followList,
            "SnapshotProjections.followList decoded nil — check CodingKeys.followList raw value matches \"chirp.followList\" (post-convertFromSnakeCase of \"chirp.follow_list\")")
    }

    /// Sanity check: the four projection fields all default to nil when the
    /// kernel emits an empty projections map (an older kernel build that
    /// predates the projections, or a fresh actor with no registrations yet).
    /// This is the steady-state any new field MUST tolerate (D1 — never
    /// fail a decode on a missing projection).
    func testEmptyProjectionsMapDecodesWithAllNils() throws {
        let projections = try snapshotDecoder().decode(
            SnapshotProjections.self, from: Data("{}".utf8))
        XCTAssertNil(projections.groupChat)
        XCTAssertNil(projections.discoveredGroups)
        XCTAssertNil(projections.dmInbox)
        XCTAssertNil(projections.followList)
    }
}
