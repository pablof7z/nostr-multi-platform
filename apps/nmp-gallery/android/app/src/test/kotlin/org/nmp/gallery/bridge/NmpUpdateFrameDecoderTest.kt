package org.nmp.gallery.bridge

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.fail
import org.junit.Test

class NmpUpdateFrameDecoderTest {

    @Test
    fun typed_snapshot_json_decodes_to_expected_payload() {
        val decoded = NmpUpdateFrameDecoder.decodeSnapshot(byteArrayOf(1, 2, 3)) {
            """
            {
              "schema_version": 1,
              "running": true,
              "projections": {
                "refs.profile": {
                  "abc": {
                    "pubkey": "abc",
                    "display_name": "Alice",
                    "npub": "npub1abc",
                    "npub_short": "npub1abc"
                  }
                }
              }
            }
            """.trimIndent()
        }

        assertEquals(true, (decoded["running"] as JsonPrimitive).content.toBoolean())
        val projections = decoded["projections"] as JsonObject
        val profiles = projections["refs.profile"] as JsonObject
        val profile = profiles["abc"] as JsonObject
        assertEquals("Alice", (profile["display_name"] as JsonPrimitive).content)
    }

    @Test
    fun provider_null_throws_invalid_flatbuffer() {
        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(ByteArray(0)) { null }
        }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidFlatbuffer, ex.kind)
    }

    @Test
    fun malformed_json_throws_invalid_value() {
        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(byteArrayOf(1)) { "{" }
        }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidValue, ex.kind)
    }

    @Test
    fun non_object_json_throws_invalid_value() {
        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(byteArrayOf(1)) { "[]" }
        }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidValue, ex.kind)
    }

    @Test
    fun schema_version_mismatch_throws() {
        val ex = expectDecodeException {
            NmpUpdateFrameDecoder.decodeSnapshot(byteArrayOf(1)) {
                """{"schema_version":99,"projections":{}}"""
            }
        }
        assertEquals(UpdateFrameDecodeErrorKind.SchemaVersionMismatch, ex.kind)
    }

    private fun expectDecodeException(block: () -> Unit): UpdateFrameDecodeException {
        try {
            block()
        } catch (e: UpdateFrameDecodeException) {
            return e
        }
        fail("expected UpdateFrameDecodeException")
        error("unreachable")
    }
}
