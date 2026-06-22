package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.ActionLifecycleSnapshot
import nmp.kernel.LifecycleEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for [TypedActionLifecycleDecoder] — the typed-first decode of the
 * kernel-owned `action_lifecycle` (`KALC`) snapshot projection (#1099). This
 * sidecar drives the in-flight / recent-terminal action UI, including the
 * Marmot dialog dismissal on a `recentTerminal` "accepted" entry — broken on
 * Android until this decoder was wired. Coverage:
 *  - one in-flight entry decodes correctly;
 *  - recent-terminal "accepted" decodes → dialog-dismissable;
 *  - an empty-but-present sidecar yields empty lists (NOT null);
 *  - absent sidecar / wrong identifier → null;
 *  - has_reason == false lifts to reason == null.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedActionLifecycleDecoderTest {

    @Test
    fun absentSidecarReturnsNull() {
        assertNull(TypedActionLifecycleDecoder.decode(emptyList()))
    }

    @Test
    fun wrongFileIdentifierReturnsNull() {
        val garbled = lifecycleBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KALC identifier
        assertNull(TypedActionLifecycleDecoder.decode(garbled))
    }

    @Test
    fun inFlightEntryDecodes() {
        val out = requireNotNull(TypedActionLifecycleDecoder.decode(lifecycleBuffer()))
        assertEquals(1, out.inFlight.size)
        assertEquals("corr-1", out.inFlight[0].correlationId)
        assertEquals("publishing", out.inFlight[0].stage)
        assertNull(out.inFlight[0].reason) // has_reason == false → null
    }

    @Test
    fun recentTerminalAcceptedIsDialogDismissable() {
        val out = requireNotNull(TypedActionLifecycleDecoder.decode(lifecycleBuffer()))
        assertEquals(1, out.recentTerminal.size)
        val terminal = out.recentTerminal[0]
        assertEquals("corr-0", terminal.correlationId)
        // The Marmot dialog dismisses on a recent-terminal "accepted" stage.
        assertEquals("accepted", terminal.stage)
        assertTrue("accepted terminal must be dismissable", terminal.stage == "accepted")
    }

    @Test
    fun emptyLifecycleYieldsEmptyListsNotNull() {
        val out = requireNotNull(TypedActionLifecycleDecoder.decode(emptyLifecycleBuffer()))
        assertTrue(out.inFlight.isEmpty())
        assertTrue(out.recentTerminal.isEmpty())
    }

    @Test
    fun failedTerminalCarriesReason() {
        val out = requireNotNull(TypedActionLifecycleDecoder.decode(failedTerminalBuffer()))
        assertEquals(1, out.recentTerminal.size)
        assertEquals("failed", out.recentTerminal[0].stage)
        assertEquals("relay rejected", out.recentTerminal[0].reason)
    }

    @Test
    fun cancelledTerminalDecodesDistinctFromFailed() {
        // S7/#1754: the DISTINCT user-initiated `cancelled` terminal decodes as
        // its own stage string — never `failed`. It carries no reason.
        val out = requireNotNull(TypedActionLifecycleDecoder.decode(cancelledTerminalBuffer()))
        assertEquals(1, out.recentTerminal.size)
        assertEquals("op-corr-cancel", out.recentTerminal[0].correlationId)
        assertEquals("cancelled", out.recentTerminal[0].stage)
        assertNull(out.recentTerminal[0].reason)
    }

    // ── builders ───────────────────────────────────────────────────────────────

    private fun entry(
        b: FlatBufferBuilder,
        correlationId: String,
        stage: String,
        reason: String?,
    ): Int {
        val corrOff = b.createString(correlationId)
        val stageOff = b.createString(stage)
        val reasonOff = if (reason != null) b.createString(reason) else 0
        return LifecycleEntry.createLifecycleEntry(
            b, corrOff, stageOff, reason != null, reasonOff, false, 0, false, 0,
        )
    }

    private fun lifecycleBuffer(): ByteArray {
        val b = FlatBufferBuilder(512)
        val inFlight = ActionLifecycleSnapshot.createInFlightVector(
            b, intArrayOf(entry(b, "corr-1", "publishing", null)),
        )
        val terminal = ActionLifecycleSnapshot.createRecentTerminalVector(
            b, intArrayOf(entry(b, "corr-0", "accepted", null)),
        )
        val snap = ActionLifecycleSnapshot.createActionLifecycleSnapshot(b, inFlight, terminal)
        ActionLifecycleSnapshot.finishActionLifecycleSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    private fun emptyLifecycleBuffer(): ByteArray {
        val b = FlatBufferBuilder(64)
        ActionLifecycleSnapshot.startActionLifecycleSnapshot(b)
        val snap = ActionLifecycleSnapshot.endActionLifecycleSnapshot(b)
        ActionLifecycleSnapshot.finishActionLifecycleSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    private fun failedTerminalBuffer(): ByteArray {
        val b = FlatBufferBuilder(256)
        val terminal = ActionLifecycleSnapshot.createRecentTerminalVector(
            b, intArrayOf(entry(b, "corr-2", "failed", "relay rejected")),
        )
        ActionLifecycleSnapshot.startActionLifecycleSnapshot(b)
        ActionLifecycleSnapshot.addRecentTerminal(b, terminal)
        val snap = ActionLifecycleSnapshot.endActionLifecycleSnapshot(b)
        ActionLifecycleSnapshot.finishActionLifecycleSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    private fun cancelledTerminalBuffer(): ByteArray {
        val b = FlatBufferBuilder(256)
        val terminal = ActionLifecycleSnapshot.createRecentTerminalVector(
            b, intArrayOf(entry(b, "op-corr-cancel", "cancelled", null)),
        )
        ActionLifecycleSnapshot.startActionLifecycleSnapshot(b)
        ActionLifecycleSnapshot.addRecentTerminal(b, terminal)
        val snap = ActionLifecycleSnapshot.endActionLifecycleSnapshot(b)
        ActionLifecycleSnapshot.finishActionLifecycleSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }
}
