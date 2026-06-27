package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Cross-language byte-parity gate for the `nmp.marmot` generated host builders
 * (M14-1c / #2169).
 *
 * Each test builds a `DispatchEnvelope` via [GeneratedActionBuilders] and asserts
 * the bytes are IDENTICAL to a canonical golden fixture. The SAME golden hex is
 * asserted by:
 *   * Rust:  `crates/nmp-marmot/src/wire/action_payload_tests.rs`
 *            (`golden_*_payload_byte_identical` — the ENVELOPE constants).
 *   * Swift: `apps/chirp/ios/ChirpTests/MarmotBuilderGoldenTests.swift`.
 *
 * This FORCES the Kotlin `flatbuffers-java` builder output to be byte-identical
 * to the Rust `MarmotAction::encode()` + `encode_dispatch_envelope` output —
 * blessing the present-empty non-optional vector encoding (relays /
 * signed_key_package_events_json) that all three sides must agree on. If the
 * Kotlin builder ever diverges (slot order, vector presence, envelope shape),
 * this test fails before the drift reaches a device.
 *
 * The fixtures are the full envelope for the fixed correlation id `"golden-corr"`.
 * To regenerate after an intentional schema change, see the regeneration note in
 * the Rust `action_payload_tests.rs` golden section, then update the
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
                "canonical golden NMPD envelope (Rust + Swift assert the SAME hex)",
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
                "NMPD envelope (Rust + Swift assert the SAME hex)",
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
