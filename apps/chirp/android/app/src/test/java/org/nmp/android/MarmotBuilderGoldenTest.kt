package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Cross-language byte-parity gate for the `nmp.marmot` generated host builders
 * (M14-1c / #2169).
 *
 * Each test builds a `DispatchEnvelope` via [GeneratedActionBuilders] and asserts
 * the bytes are byte-IDENTICAL to a canonical host fixture. The SAME fixture is
 * asserted by the Swift shell:
 *   * Swift: `apps/chirp/ios/ChirpTests/MarmotBuilderGoldenTests.swift`.
 * So this gate proves Kotlin↔Swift builder byte-identity — the meaningful
 * cross-SHELL guarantee. (The two hand-rolled host builders are byte-identical
 * to each other.)
 *
 * Rust parity is SEMANTIC, not byte-identical: the Rust `MarmotAction::encode()`
 * uses the flatc-generated `*::create()` builders, which pack table fields in a
 * different order than these hand-rolled forward-slot builders. The bytes
 * therefore differ while decoding to the identical `MarmotAction` (FlatBuffers
 * decodes by vtable slot, order-independent). The Rust test
 * `host_builder_bytes_round_trip_to_expected_action`
 * (`crates/nmp-marmot/src/wire/action_payload_tests.rs`) feeds THESE exact host
 * bytes through the production decode path and asserts the expected
 * `MarmotAction`, including the present-empty non-optional vectors (relays /
 * signedKeyPackageEventsJson) that #2169 blesses. See that file's
 * "parity contract" comment for the full rationale.
 *
 * If the Kotlin builder ever diverges (slot order, vector presence, envelope
 * shape), this test fails before the drift reaches a device.
 *
 * The fixtures are the full envelope for the fixed correlation id `"golden-corr"`.
 * To regenerate after an intentional schema change, see the regeneration note in
 * the Rust `action_payload_tests.rs` parity-contract section, then update the
 * `*.fb.hex` fixtures here and the Swift constants.
 *
 * This file REPLACES the obsolete `MarmotActionEnvelopesTest.kt`, which tested
 * the deleted JSON DTO path (the JSON doorway is gone — #2169).
 */
class MarmotBuilderGoldenTest {

    @Test
    fun publishKeyPackageEmptyRelays_isByteIdenticalToGolden() {
        val golden = loadFixture("marmot_publish_key_package_empty_golden_v1.fb.hex")
        val actual = GeneratedActionBuilders.marmotPublishKeyPackage(
            correlationId = GOLDEN_CORRELATION_ID,
            relays = emptyList(),
        )
        assertEquals(
            "marmotPublishKeyPackage(relays=[]) must be byte-identical to the " +
                "canonical host NMPD envelope fixture (Swift asserts the SAME hex; " +
                    "Rust round-trips these exact bytes — see the parity contract)",
            golden,
            toHex(actual),
        )
    }

    @Test
    fun createGroupPopulated_isByteIdenticalToGolden() {
        val golden = loadFixture("marmot_create_group_populated_golden_v1.fb.hex")
        val actual = GeneratedActionBuilders.marmotCreateGroup(
            correlationId = GOLDEN_CORRELATION_ID,
            name = "Engineering",
            description = "Team chat",
            inviteeText = "npub1abc npub1def",
            inviteeNpubs = listOf("npub1abc", "npub1def"),
            signedKeyPackageEventsJson = emptyList(),
            relays = listOf("wss://relay.example"),
        )
        assertEquals(
            "marmotCreateGroup(..) must be byte-identical to the canonical golden " +
                "host NMPD envelope fixture (Swift asserts the SAME hex; Rust " +
                    "round-trips these exact bytes — see the parity contract)",
            golden,
            toHex(actual),
        )
    }

    private fun loadFixture(name: String): String =
        javaClass.classLoader
            ?.getResourceAsStream("fixtures/$name")
            ?.bufferedReader()
            ?.readText()
            ?.trim()
            ?: error("fixture not found on classpath: $name")

    private fun toHex(bytes: ByteArray): String =
        bytes.joinToString("") { "%02x".format(it) }

    private companion object {
        /** The fixed correlation id baked into the golden envelope fixtures. */
        const val GOLDEN_CORRELATION_ID = "golden-corr"
    }
}
