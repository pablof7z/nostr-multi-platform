package org.nmp.gallery.bridge

import com.google.flatbuffers.FlatBufferBuilder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import nmp.transport.FrameKind
import nmp.transport.Pair as FbPair
import nmp.transport.PanicFrame
import nmp.transport.SnapshotFrame
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class NmpUpdateFrameDecoderTest {

    // Canonical fixture lives at crates/nmp-core/tests/fixtures/update_frame_snapshot_v1.fb.hex
    // and is mirrored into src/test/resources/fixtures/ to keep the test classpath-relative.
    // Keep the two copies in sync manually — no CI gate exists today.
    private fun loadFixtureBytes(): ByteArray {
        val stream = javaClass.classLoader!!.getResourceAsStream(
            "fixtures/update_frame_snapshot_v1.fb.hex",
        ) ?: error("fixture missing from test classpath")
        val hex = stream.bufferedReader().readText().filter { !it.isWhitespace() }
        require(hex.length % 2 == 0)
        val out = ByteArray(hex.length / 2)
        for (i in out.indices) {
            out[i] = hex.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
        return out
    }

    @Test
    fun canonical_fixture_decodes_to_expected_payload() {
        val bytes = loadFixtureBytes()
        val decoded = NmpUpdateFrameDecoder.decodeSnapshot(bytes)

        val expected = Json.parseToJsonElement(
            """
            {
              "schema_version": 1,
              "rev": 42,
              "running": true,
              "projections": { "timeline": [{ "id": "a", "score": 1.5 }] }
            }
            """.trimIndent(),
        ) as JsonObject

        assertEquals(expected, decoded)
    }

    @Test
    fun missing_identifier_throws_invalid_flatbuffer() {
        val bytes = ByteArray(32)
        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidFlatbuffer, ex.kind)
    }

    @Test
    fun panic_frame_throws_unexpected_panic() {
        val builder = FlatBufferBuilder()
        val msg = builder.createString("actor exploded")
        val panic = PanicFrame.createPanicFrame(builder, msg)
        val root = UpdateFrame.createUpdateFrame(builder, FrameKind.Panic, 0, panic)
        UpdateFrame.finishUpdateFrameBuffer(builder, root)
        val bytes = builder.sizedByteArray()

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.UnexpectedPanicFrame, ex.kind)
        assertTrue(ex.message!!.contains("actor exploded"))
    }

    @Test
    fun snapshot_schema_version_mismatch_throws() {
        val bytes = buildSnapshotFrame(
            schemaVersion = 99u,
            payloadBuilder = { builder ->
                Value.createValue(
                    builder,
                    ValueKind.Map,
                    false, 0L, 0UL, 0.0,
                    0, 0, Value.createMapVector(builder, IntArray(0)),
                )
            },
        )

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.SchemaVersionMismatch, ex.kind)
    }

    @Test
    fun inner_schema_version_mismatch_throws() {
        val bytes = buildSnapshotFrame(
            schemaVersion = 1u,
            payloadBuilder = { builder -> mapWithSchemaVersion(builder, 7L) },
        )

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.SchemaVersionMismatch, ex.kind)
    }

    @Test
    fun unknown_value_kind_throws_invalid_value() {
        val unknownKind: UByte = 99u
        val bytes = buildSnapshotFrame(
            schemaVersion = 1u,
            payloadBuilder = { builder ->
                // Root must be a map; embed an unknown-kind value as a sole entry.
                val key = builder.createString("k")
                val bad = Value.createValue(
                    builder,
                    unknownKind,
                    false, 0L, 0UL, 0.0, 0, 0, 0,
                )
                val pair = FbPair.createPair(builder, key, bad)
                val mapVec = Value.createMapVector(builder, intArrayOf(pair))
                Value.createValue(
                    builder,
                    ValueKind.Map,
                    false, 0L, 0UL, 0.0, 0, 0, mapVec,
                )
            },
        )

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidValue, ex.kind)
    }

    @Test
    fun non_finite_float_throws_invalid_value() {
        val bytes = buildSnapshotFrame(
            schemaVersion = 1u,
            payloadBuilder = { builder ->
                val key = builder.createString("k")
                val nanValue = Value.createValue(
                    builder,
                    ValueKind.Float,
                    false, 0L, 0UL, Double.NaN, 0, 0, 0,
                )
                val pair = FbPair.createPair(builder, key, nanValue)
                val mapVec = Value.createMapVector(builder, intArrayOf(pair))
                Value.createValue(
                    builder,
                    ValueKind.Map,
                    false, 0L, 0UL, 0.0, 0, 0, mapVec,
                )
            },
        )

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.InvalidValue, ex.kind)
    }

    @Test
    fun large_unsigned_preserves_precision_as_unquoted_literal() {
        val largeUnsigned: ULong = ULong.MAX_VALUE - 1u
        val bytes = buildSnapshotFrame(
            schemaVersion = 1u,
            payloadBuilder = { builder ->
                val key = builder.createString("bytes_rx")
                val bigValue = Value.createValue(
                    builder,
                    ValueKind.UInt,
                    false, 0L, largeUnsigned, 0.0, 0, 0, 0,
                )
                val pair = FbPair.createPair(builder, key, bigValue)
                val mapVec = Value.createMapVector(builder, intArrayOf(pair))
                Value.createValue(
                    builder,
                    ValueKind.Map,
                    false, 0L, 0UL, 0.0, 0, 0, mapVec,
                )
            },
        )

        val decoded = NmpUpdateFrameDecoder.decodeSnapshot(bytes)
        val raw = decoded["bytes_rx"]
        assertNotNull(raw)
        // Equality on JsonElement compares the underlying content. The decimal
        // representation must round-trip without precision loss.
        assertEquals(largeUnsigned.toString(), raw!!.toString())
    }

    @Test
    fun snapshot_missing_payload_throws() {
        val builder = FlatBufferBuilder()
        SnapshotFrame.startSnapshotFrame(builder)
        SnapshotFrame.addSchemaVersion(builder, 1u)
        val snapshot = SnapshotFrame.endSnapshotFrame(builder)
        val root = UpdateFrame.createUpdateFrame(builder, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(builder, root)
        val bytes = builder.sizedByteArray()

        val ex = expectDecodeException { NmpUpdateFrameDecoder.decodeSnapshot(bytes) }
        assertEquals(UpdateFrameDecodeErrorKind.MissingSnapshotPayload, ex.kind)
    }

    private fun buildSnapshotFrame(
        schemaVersion: UInt,
        payloadBuilder: (FlatBufferBuilder) -> Int,
    ): ByteArray {
        val builder = FlatBufferBuilder()
        val payload = payloadBuilder(builder)
        val snapshot = SnapshotFrame.createSnapshotFrame(builder, schemaVersion, payload)
        val root = UpdateFrame.createUpdateFrame(builder, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(builder, root)
        return builder.sizedByteArray()
    }

    private fun mapWithSchemaVersion(builder: FlatBufferBuilder, version: Long): Int {
        val key = builder.createString("schema_version")
        val value = Value.createValue(
            builder,
            ValueKind.Int,
            false, version, 0UL, 0.0, 0, 0, 0,
        )
        val pair = FbPair.createPair(builder, key, value)
        val mapVec = Value.createMapVector(builder, intArrayOf(pair))
        return Value.createValue(
            builder,
            ValueKind.Map,
            false, 0L, 0UL, 0.0, 0, 0, mapVec,
        )
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
