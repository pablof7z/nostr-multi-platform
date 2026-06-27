import XCTest
@testable import Chirp

/// Cross-language byte-parity gate for the `nmp.marmot` generated host builders
/// (M14-1c / #2169).
///
/// Each test builds a `DispatchEnvelope` via `GeneratedActionBuilders` and
/// asserts the bytes are IDENTICAL to a canonical golden fixture. The SAME
/// golden hex is asserted by:
///   * Rust:   `crates/nmp-marmot/src/wire/action_payload_tests.rs`
///             (`golden_*_payload_byte_identical` — the ENVELOPE constants).
///   * Kotlin: `apps/chirp/android/.../MarmotBuilderGoldenTest.kt`
///             (+ `*_golden_v1.fb.hex` fixtures).
///
/// This FORCES the Swift `FlatBuffers` builder output to be byte-identical to
/// the Rust `MarmotAction::encode()` + `encode_dispatch_envelope` output —
/// blessing the present-empty non-optional vector encoding (relays /
/// signed_key_package_events_json) that all three sides must agree on. If the
/// Swift builder ever diverges (slot order, vector presence, envelope shape),
/// this test fails before the drift reaches a device.
///
/// The fixtures are the full envelope for the fixed correlation id `"golden-corr"`.
final class MarmotBuilderGoldenTests: XCTestCase {

    /// The fixed correlation id baked into the golden envelope fixtures.
    private let goldenCorrelationID = "golden-corr"

    /// `marmotPublishKeyPackage(relays: [])` — the EMPTY-vector arm. `relays` is
    /// a NON-OPTIONAL `[string]` emitted as a PRESENT empty vector.
    private let goldenPublishKeyPackageEmpty =
        "140000004e4d50440c001400040008000c0010000c0000005c00000048000000010000000400000038000000140000004e4d4d4100000a001200080007000c000a00000000000001010000000c00000000000600080004000600000004000000000000000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200"

    /// `marmotCreateGroup(..)` — the POPULATED arm.
    private let goldenCreateGroupPopulated =
        "140000004e4d50440c001400040008000c0010000c000000fc000000e80000000100000004000000d8000000140000004e4d4d4100000a001000080007000c000a00000000000002010000001400000010001c00040008000c0010001400180010000000180000002400000030000000440000006c0000006c0000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c65000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200"

    func testPublishKeyPackageEmptyRelaysIsByteIdenticalToGolden() {
        let bytes = GeneratedActionBuilders.marmotPublishKeyPackage(
            correlationId: goldenCorrelationID,
            relays: []
        )
        XCTAssertEqual(
            hex(bytes),
            goldenPublishKeyPackageEmpty,
            "marmotPublishKeyPackage(relays: []) must be byte-identical to the "
                + "canonical golden NMPD envelope (Rust + Kotlin assert the SAME hex)"
        )
    }

    func testCreateGroupPopulatedIsByteIdenticalToGolden() {
        let bytes = GeneratedActionBuilders.marmotCreateGroup(
            correlationId: goldenCorrelationID,
            name: "Engineering",
            description: "Team chat",
            inviteeText: "npub1abc npub1def",
            inviteeNpubs: ["npub1abc", "npub1def"],
            signedKeyPackageEventsJson: [],
            relays: ["wss://relay.example"]
        )
        XCTAssertEqual(
            hex(bytes),
            goldenCreateGroupPopulated,
            "marmotCreateGroup(..) must be byte-identical to the canonical golden "
                + "NMPD envelope (Rust + Kotlin assert the SAME hex)"
        )
    }

    private func hex(_ bytes: [UInt8]) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }
}
