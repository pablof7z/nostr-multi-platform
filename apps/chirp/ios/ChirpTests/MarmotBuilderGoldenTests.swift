import XCTest
@testable import Chirp

/// Cross-language byte-parity gate for the `nmp.marmot` generated host builders
/// (M14-1c / #2169).
///
/// Each test builds a `DispatchEnvelope` via `GeneratedActionBuilders` and
/// asserts the bytes are byte-IDENTICAL to a canonical host fixture. The SAME
/// fixture is asserted by the Kotlin shell:
///   * Kotlin: `apps/chirp/android/.../MarmotBuilderGoldenTest.kt`
///             (+ `*_golden_v1.fb.hex` fixtures).
/// So this gate proves Swift↔Kotlin builder byte-identity — the meaningful
/// cross-SHELL guarantee. (The two hand-rolled host builders are byte-identical
/// to each other.)
///
/// Rust parity is SEMANTIC, not byte-identical: the Rust `MarmotAction::encode()`
/// uses the flatc-generated `*::create()` builders, which pack table fields in a
/// different order than these hand-rolled forward-slot builders. The bytes
/// therefore differ while decoding to the identical `MarmotAction` (FlatBuffers
/// decodes by vtable slot, order-independent). The Rust test
/// `host_builder_bytes_round_trip_to_expected_action`
/// (`crates/nmp-marmot/src/wire/action_payload_tests.rs`) feeds THESE exact host
/// bytes through the production decode path and asserts the expected
/// `MarmotAction`, including the present-empty non-optional vectors (relays /
/// signedKeyPackageEventsJson) that #2169 blesses. See that file's
/// "parity contract" comment for the full rationale.
///
/// If the Swift builder ever diverges (slot order, vector presence, envelope
/// shape), this test fails before the drift reaches a device.
///
/// The fixtures are the full envelope for the fixed correlation id `"golden-corr"`.
final class MarmotBuilderGoldenTests: XCTestCase {

    /// The fixed correlation id baked into the golden envelope fixtures.
    private let goldenCorrelationID = "golden-corr"

    /// `marmotPublishKeyPackage(relays: [])` — the EMPTY-vector arm. `relays` is
    /// a NON-OPTIONAL `[string]` emitted as a PRESENT empty vector.
    private let goldenPublishKeyPackageEmpty =
        "140000004e4d50440c00140010000c00080004000c0000001000000001000000440000005000000038000000140000004e4d4d4100000a0012000c000b0004000a00000014000000000000010100000000000600080004000600000004000000000000000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200"

    /// `marmotCreateGroup(..)` — the POPULATED arm.
    private let goldenCreateGroupPopulated =
        "140000004e4d50440c00140010000c00080004000c0000001000000001000000e4000000f0000000d8000000140000004e4d4d4100000a0010000c000b0004000a0000001c000000000000020100000010001c001800140010000c0008000400100000008000000078000000480000002c00000018000000040000000b000000456e67696e656572696e6700090000005465616d2063686174000000110000006e70756231616263206e70756231646566000000020000001800000004000000080000006e7075623164656600000000080000006e7075623161626300000000000000000100000004000000130000007773733a2f2f72656c61792e6578616d706c65000a0000006e6d702e6d61726d6f7400000b000000676f6c64656e2d636f727200"

    func testPublishKeyPackageEmptyRelaysIsByteIdenticalToGolden() {
        let bytes = GeneratedActionBuilders.marmotPublishKeyPackage(
            correlationId: goldenCorrelationID,
            relays: []
        )
        XCTAssertEqual(
            hex(bytes),
            goldenPublishKeyPackageEmpty,
            "marmotPublishKeyPackage(relays: []) must be byte-identical to the "
                + "canonical host NMPD envelope fixture (Kotlin asserts the SAME hex; "
                + "Rust round-trips these exact bytes — see the parity contract)"
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
            "marmotCreateGroup(..) must be byte-identical to the canonical host "
                + "NMPD envelope fixture (Kotlin asserts the SAME hex; Rust "
                + "round-trips these exact bytes — see the parity contract)"
        )
    }

    private func hex(_ bytes: [UInt8]) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }
}
