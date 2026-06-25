package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.RelayRoleOption as FbRelayRoleOption
import nmp.kernel.RelayRoleOptionsSnapshot
import nmp.transport.FrameKind
import nmp.transport.Metrics
import nmp.transport.ProjectionPresenceState
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Integration tests for the PR-B typed-first projection wiring in
 * [KernelUpdateFrameDecoder.decodeProjections] (#979 / #1084): verify that
 *
 *  1. when NO typed sidecar is present, each projection is absent (empty/null) —
 *     the generic `payload:Value` path is gone (PR-B #991/#979); absence of a
 *     sidecar means no data;
 *  2. when a typed sidecar IS present, it populates the projection correctly;
 *  3. when a typed sidecar is garbled / undecodable, the projection is absent
 *     (no crash, fails closed per D1).
 *
 * Built end-to-end through `KernelUpdateFrameDecoder.decode(bytes)` so the full
 * snapshot → Tier-3 envelope → typed-projection-lift → projection-decode path
 * is exercised. Frames are assembled via the Tier-3 SnapshotFrame builder
 * (ADR-0044; #1084 regression guard).
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedProjectionFallbackTest {

    @Test
    fun noTypedSidecarYieldsEmptyRelayRoleOptions() {
        // PR-B: without a typed sidecar, relay_role_options is empty (no generic
        // payload fallback exists any more).
        val frame = frame(rev = 5L, typedSidecars = emptyList())
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        assertTrue("no typed sidecar → relay_role_options must be empty", opts.isEmpty())
    }

    @Test
    fun typedRelayRoleOptionsAreDecoded() {
        val typedBytes = relayRoleSidecarBytes()
        val frame = frame(
            rev = 6L,
            typedSidecars = listOf(
                Triple("relay_role_options", "relay_role_options", typedBytes),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        assertEquals(listOf("both", "read"), opts.map { it.value })
        assertTrue(opts[0].isDefault)
    }

    @Test
    fun malformedTypedSidecarYieldsEmptyRelayRoleOptions() {
        // A garbled sidecar must not crash and must yield an empty list.
        val garbled = relayRoleSidecarBytes().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KRRO identifier → undecodable
        val frame = frame(
            rev = 7L,
            typedSidecars = listOf(
                Triple("relay_role_options", "relay_role_options", garbled),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val opts = decoded.update.projections?.relayRoleOptions.orEmpty()
        // Undecodable typed sidecar → fails closed, no crash, empty result.
        assertTrue("garbled sidecar must yield empty relay_role_options", opts.isEmpty())
    }

    @Test
    fun tier3RevAndRunningAreReadFromEnvelopeNotPayload() {
        // Regression test for #1084: rev and running come from the Tier-3
        // SnapshotFrame fields, not from the (gone) `payload:Value` root map.
        val frame = frame(rev = 42L, running = true, typedSidecars = emptyList())
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        assertEquals("rev must come from Tier-3 envelope", 42L, decoded.update.rev)
        assertTrue("running must come from Tier-3 envelope", decoded.update.running)
    }

    @Test
    fun tier3MetricsAreDecoded() {
        val frame = frame(
            rev = 1L,
            storedEvents = 999UL,
            visibleItems = 42UL,
            eventsRx = 7UL,
            updateSequence = 11UL,
            typedSidecars = emptyList(),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val metrics = decoded.update.metrics
        assertEquals("storedEvents from Tier-3 metrics", 999L, metrics?.storedEvents)
        assertEquals("visibleItems from Tier-3 metrics", 42L, metrics?.visibleItems)
        assertEquals("eventsRx from Tier-3 metrics", 7L, metrics?.eventsRx)
        assertEquals("updateSequence from Tier-3 metrics", 11L, metrics?.updateSequence)
    }

    @Test
    fun tier3LastErrorToastIsDecoded() {
        val frame = frame(rev = 1L, lastErrorToast = "boom", typedSidecars = emptyList())
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        assertEquals("boom", decoded.update.lastErrorToast)
    }

    @Test
    fun tier3AbsentLastErrorToastIsNull() {
        val frame = frame(rev = 1L, typedSidecars = emptyList())
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        assertNull(decoded.update.lastErrorToast)
    }

    // ── builders ─────────────────────────────────────────────────────────────

    private fun relayRoleSidecarBytes(): ByteArray {
        val b = FlatBufferBuilder(256)
        fun opt(value: String, tint: String, isDefault: Boolean): Int {
            val v = b.createString(value)
            val t = b.createString(tint)
            return FbRelayRoleOption.createRelayRoleOption(b, v, t, isDefault)
        }
        val vec = RelayRoleOptionsSnapshot.createOptionsVector(b, intArrayOf(opt("both", "accent", true), opt("read", "info", false)))
        val snap = RelayRoleOptionsSnapshot.createRelayRoleOptionsSnapshot(b, vec)
        RelayRoleOptionsSnapshot.finishRelayRoleOptionsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /**
     * Build a minimal Tier-3 snapshot frame. `rev` and `running` go directly
     * into the SnapshotFrame envelope (ADR-0044). `typedSidecars` are embedded in
     * the `typed_projections` vector. The metrics table is always written (the
     * iOS `extractTypedEnvelope` gates on `metrics != nil`; mirrored here).
     */
    private fun frame(
        rev: Long = 1L,
        running: Boolean = false,
        storedEvents: ULong = 0UL,
        visibleItems: ULong = 0UL,
        eventsRx: ULong = 0UL,
        updateSequence: ULong = 0UL,
        lastErrorToast: String? = null,
        typedSidecars: List<Triple<String, String, ByteArray>>,
    ): ByteArray {
        val b = FlatBufferBuilder(2048)
        // Build typed sidecars first (must precede table construction per FlatBuffers ordering).
        val sidecarOffsets = typedSidecars.map { (key, schemaId, bytes) ->
            typedProjection(b, key, schemaId, bytes)
        }.toIntArray()
        val typedVec = SnapshotFrame.createTypedProjectionsVector(b, sidecarOffsets)

        // Metrics table — always present (mirrors production frames).
        val metricsOffset = buildMetrics(b,
            storedEvents = storedEvents,
            visibleItems = visibleItems,
            eventsRx = eventsRx,
            updateSequence = updateSequence,
        )
        val lastErrorToastOffset = lastErrorToast?.let { b.createString(it) } ?: 0

        val snapshot = SnapshotFrame.createSnapshotFrame(
            b,
            /* schemaVersion = */ 1u,
            /* typedProjectionsOffset = */ typedVec,
            /* rev = */ rev.toULong(),
            /* kernelSchemaVersion = */ 0u,
            /* lastTickMs = */ 0UL,
            /* updateKindOffset = */ 0,
            /* running = */ running,
            /* metricsOffset = */ metricsOffset,
            /* relayStatusOffset = */ 0,
            /* relayStatusesOffset = */ 0,
            /* logicalInterestsOffset = */ 0,
            /* wireSubscriptionsOffset = */ 0,
            /* logsOffset = */ 0,
            /* lastErrorToastOffset = */ lastErrorToastOffset,
            /* lastErrorCategoryOffset = */ 0,
            /* lastPlannerErrorOffset = */ 0,
            /* storeOpenFailureOffset = */ 0,
            /* noConfiguredRelays = */ null,
            /* negentropySyncStatsOffset = */ 0,
            /* snapshotEpoch = */ 0UL, /* sessionId = */ 0UL,
        )
        val frame = UpdateFrame.createUpdateFrame(b, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(b, frame)
        return b.sizedByteArray()
    }

    private fun buildMetrics(
        b: FlatBufferBuilder,
        storedEvents: ULong,
        visibleItems: ULong,
        eventsRx: ULong,
        updateSequence: ULong,
    ): Int {
        Metrics.startMetrics(b)
        Metrics.addStoredEvents(b, storedEvents)
        Metrics.addVisibleItems(b, visibleItems)
        Metrics.addEventsRx(b, eventsRx)
        Metrics.addUpdateSequence(b, updateSequence)
        return Metrics.endMetrics(b)
    }

    private fun typedProjection(b: FlatBufferBuilder, key: String, schemaId: String, bytes: ByteArray): Int {
        val keyOffset = b.createString(key)
        val schemaIdOffset = b.createString(schemaId)
        val fileIdOffset = b.createString("KRRO")
        val payloadVec = TypedPayload.createPayloadVector(b, bytes.toUByteArray())
        val typedPayload = TypedPayload.createTypedPayload(b, schemaIdOffset, 1u, fileIdOffset, payloadVec)
        return TypedProjection.createTypedProjection(
            b, keyOffset, typedPayload,
            /* projectionRev = */ 0UL, /* state = */ ProjectionPresenceState.Changed,
        )
    }
}
