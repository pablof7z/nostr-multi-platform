package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.RelayRoleOption as FbRelayRoleOption
import nmp.kernel.RelayRoleOptionsSnapshot
import nmp.transport.FrameKind
import nmp.transport.Pair as TransportPair
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Integration tests for the F-05 typed-first projection wiring in
 * [KernelUpdateFrameDecoder.decodeProjections] (#979): verify that
 *
 *  1. when NO typed sidecar is present, the generic `payload:Value` path still
 *     populates each projection (ADR-0037 Commitment 4 — permanent fallback);
 *  2. when a typed sidecar IS present, it wins over the generic subtree.
 *
 * Built end-to-end through `KernelUpdateFrameDecoder.decode(bytes)` so the whole
 * snapshot → typed-projection-lift → projection-decode path is exercised.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedProjectionFallbackTest {

    @Test
    fun genericRelayRoleOptionsSurviveWithoutTypedSidecar() {
        val frame = frame(
            rev = 5L,
            projections = { b ->
                valueMap(
                    b,
                    "relay_role_options" to valueList(
                        b,
                        relayRoleOptionEntry(b, "both", "Both", "accent", true),
                        relayRoleOptionEntry(b, "read", "Read", "info", false),
                    ),
                )
            },
            typedSidecars = emptyList(),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        // Generic path survived (no typed sidecar present).
        assertEquals(listOf("both", "read"), opts.map { it.value })
        assertTrue(opts[0].isDefault)
    }

    @Test
    fun typedRelayRoleOptionsWinOverGeneric() {
        // Generic carries ONE option; typed sidecar carries TWO. Typed must win.
        val typedBytes = relayRoleSidecarBytes()
        val frame = frame(
            rev = 6L,
            projections = { b ->
                valueMap(
                    b,
                    "relay_role_options" to valueList(
                        b,
                        relayRoleOptionEntry(b, "generic-only", "Generic", "neutral", false),
                    ),
                )
            },
            typedSidecars = listOf(
                Triple("relay_role_options", "relay_role_options", typedBytes),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        // Typed sidecar (two options) replaced the single-entry generic subtree.
        assertEquals(listOf("both", "read"), opts.map { it.value })
    }

    @Test
    fun malformedTypedSidecarFallsBackToGeneric() {
        val garbled = relayRoleSidecarBytes().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KRRO identifier → undecodable
        val frame = frame(
            rev = 7L,
            projections = { b ->
                valueMap(
                    b,
                    "relay_role_options" to valueList(
                        b,
                        relayRoleOptionEntry(b, "fallback", "Fallback", "neutral", true),
                    ),
                )
            },
            typedSidecars = listOf(
                Triple("relay_role_options", "relay_role_options", garbled),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        // Undecodable typed sidecar → generic subtree wins (single entry).
        assertEquals(listOf("fallback"), opts.map { it.value })
    }

    // ── builders ─────────────────────────────────────────────────────────────

    private fun relayRoleSidecarBytes(): ByteArray {
        val b = FlatBufferBuilder(256)
        fun opt(value: String, label: String, tint: String, isDefault: Boolean): Int {
            val v = b.createString(value)
            val l = b.createString(label)
            val t = b.createString(tint)
            return FbRelayRoleOption.createRelayRoleOption(b, v, l, t, isDefault)
        }
        val vec = RelayRoleOptionsSnapshot.createOptionsVector(b, intArrayOf(opt("both", "Both", "accent", true), opt("read", "Read", "info", false)))
        val snap = RelayRoleOptionsSnapshot.createRelayRoleOptionsSnapshot(b, vec)
        RelayRoleOptionsSnapshot.finishRelayRoleOptionsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** One generic `RelayRoleOption` as a `Value` map (snake_case keys). */
    private fun relayRoleOptionEntry(
        b: FlatBufferBuilder,
        value: String,
        label: String,
        tint: String,
        isDefault: Boolean,
    ): Int = valueMap(
        b,
        "value" to valueString(b, value),
        "label" to valueString(b, label),
        "tint" to valueString(b, tint),
        "is_default" to valueBool(b, isDefault),
    )

    private fun frame(
        rev: Long,
        projections: (FlatBufferBuilder) -> Int,
        typedSidecars: List<Triple<String, String, ByteArray>>,
    ): ByteArray {
        val b = FlatBufferBuilder(2048)
        val payload = valueMap(
            b,
            "rev" to valueInt(b, rev),
            "running" to valueBool(b, true),
            "relay_url" to valueString(b, ""),
            "projections" to projections(b),
        )
        val sidecarOffsets = typedSidecars.map { (key, schemaId, bytes) ->
            typedProjection(b, key, schemaId, bytes)
        }.toIntArray()
        val typedVec = SnapshotFrame.createTypedProjectionsVector(b, sidecarOffsets)
        val snapshot = SnapshotFrame.createSnapshotFrame(b, 1u, payload, typedVec)
        val frame = UpdateFrame.createUpdateFrame(b, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(b, frame)
        return b.sizedByteArray()
    }

    private fun typedProjection(b: FlatBufferBuilder, key: String, schemaId: String, bytes: ByteArray): Int {
        val keyOffset = b.createString(key)
        val schemaIdOffset = b.createString(schemaId)
        val fileIdOffset = b.createString("KRRO")
        val payloadVec = TypedPayload.createPayloadVector(b, bytes.toUByteArray())
        val typedPayload = TypedPayload.createTypedPayload(b, schemaIdOffset, 1u, fileIdOffset, payloadVec)
        return TypedProjection.createTypedProjection(b, keyOffset, typedPayload)
    }

    private fun valueString(b: FlatBufferBuilder, value: String): Int {
        val s = b.createString(value)
        return Value.createValue(b, ValueKind.String, false, 0L, 0UL, 0.0, s, 0, 0)
    }

    private fun valueInt(b: FlatBufferBuilder, value: Long): Int =
        Value.createValue(b, ValueKind.Int, false, value, 0UL, 0.0, 0, 0, 0)

    private fun valueBool(b: FlatBufferBuilder, value: Boolean): Int =
        Value.createValue(b, ValueKind.Bool, value, 0L, 0UL, 0.0, 0, 0, 0)

    private fun valueList(b: FlatBufferBuilder, vararg values: Int): Int {
        val list = Value.createListVector(b, values)
        return Value.createValue(b, ValueKind.List, false, 0L, 0UL, 0.0, 0, list, 0)
    }

    private fun valueMap(b: FlatBufferBuilder, vararg entries: Pair<String, Int>): Int {
        val pairs = entries.map { (key, value) ->
            TransportPair.createPair(b, b.createString(key), value)
        }.toIntArray()
        val map = Value.createMapVector(b, pairs)
        return Value.createValue(b, ValueKind.Map, false, 0L, 0UL, 0.0, 0, 0, map)
    }
}
