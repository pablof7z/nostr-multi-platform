package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.refs.RefRow
import nmp.refs.RefRowDeltaBatch
import nmp.refs.RefRowState
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * ADR-0063 Lane A (#1671) — invariant tests for the generated [KeyedRefCache],
 * at the generated-cache layer. Mirrors the Rust gate
 * `crates/nmp-core/src/refs/tests.rs` and the Swift twin
 * `ios/Chirp/ChirpTests/KeyedRefCacheTests.swift`.
 *
 * Each test builds REAL `nmp.refs.RefRowDeltaBatch` bytes (the kernel wire form)
 * and feeds them to [KeyedRefCache.merge] — invariants verified against
 * serialized bytes, not literals.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class KeyedRefCacheTest {
    private val profile = "refs.profile"
    private val event = "refs.event"

    // ADR-0063 Lane C (#1671): the cache exposes NO public per-namespace refs
    // accessor (the dishonest raw `ByteArray?` `profile()`/`event()` surface was
    // removed; the TYPED kotlin accessors land in Lane G once the `flatc --kotlin`
    // row readers ship). These same-module helpers read the cached row bytes via
    // the `internal payload(projectionKey, rowKey)` merge primitive so the
    // invariant assertions stay byte-faithful without re-introducing a public raw
    // surface.
    private fun KeyedRefCache.profileBytes(key: String): ByteArray? = payload(profile, key)
    private fun KeyedRefCache.eventBytes(key: String): ByteArray? = payload(event, key)

    private data class Row(val key: String, val rev: ULong, val state: UByte, val payload: ByteArray)

    private fun changed(key: String, rev: ULong, tag: Byte) =
        Row(key, rev, RefRowState.Changed, byteArrayOf(0x01, tag))

    private fun cleared(key: String, rev: ULong) =
        Row(key, rev, RefRowState.Cleared, ByteArray(0))

    private fun makeBatch(namespace: String, baseline: Boolean, rows: List<Row>): ByteArray {
        val builder = FlatBufferBuilder(256)
        val rowOffsets = IntArray(rows.size)
        for ((i, row) in rows.withIndex()) {
            val keyOff = builder.createString(row.key)
            val payOff = if (row.payload.isEmpty()) 0 else builder.createByteVector(row.payload)
            rowOffsets[i] = RefRow.createRefRow(builder, keyOff, row.rev, row.state, payOff)
        }
        val rowsVec = RefRowDeltaBatch.createRowsVector(builder, rowOffsets)
        val nsOff = builder.createString(namespace)
        val batch = RefRowDeltaBatch.createRefRowDeltaBatch(builder, nsOff, baseline, rowsVec)
        RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, batch)
        return builder.sizedByteArray()
    }

    // Invariant #1: absence is Unchanged, never Cleared.
    @Test
    fun absentRowIsRetained() {
        val cache = KeyedRefCache()
        cache.merge(
            profile,
            makeBatch("profile", true, listOf(changed("alice", 1u, 0xAA.toByte()), changed("bob", 1u, 0xBB.toByte()))),
            1u, 0u,
        )
        assertEquals(2, cache.count(profile))

        val changedKeys = cache.merge(
            profile, makeBatch("profile", false, listOf(changed("alice", 2u, 0xCC.toByte()))), 1u, 0u,
        )
        assertEquals(setOf("alice"), changedKeys)
        assertArrayEquals(byteArrayOf(0x01, 0xBB.toByte()), cache.profileBytes("bob"))
        assertArrayEquals(byteArrayOf(0x01, 0xCC.toByte()), cache.profileBytes("alice"))
    }

    // Invariant #2: decode-before-commit keeps prior on malformed, latches resync.
    @Test
    fun malformedRowKeepsPriorAndLatchesResync() {
        val cache = KeyedRefCache()
        cache.merge(
            profile,
            makeBatch("profile", true, listOf(changed("alice", 1u, 0xAA.toByte()), changed("bob", 1u, 0xBB.toByte()))),
            1u, 0u,
        )
        val batch = makeBatch(
            "profile", false,
            listOf(Row("alice", 2u, RefRowState.Changed, ByteArray(0)), changed("bob", 2u, 0xEE.toByte())),
        )
        val changedKeys = cache.merge(profile, batch, 1u, 0u)
        assertTrue(cache.needsResync)
        assertFalse(changedKeys.contains("alice"))
        assertArrayEquals(byteArrayOf(0x01, 0xAA.toByte()), cache.profileBytes("alice"))
        assertArrayEquals(byteArrayOf(0x01, 0xEE.toByte()), cache.profileBytes("bob"))
    }

    // Invariant #3: epoch change → baseline reconstructs full set.
    @Test
    fun epochBaselineRebuildsFullSet() {
        val cache = KeyedRefCache()
        cache.merge(
            profile,
            makeBatch("profile", true, listOf(changed("alice", 1u, 0xAA.toByte()), changed("ghost", 1u, 0x66))),
            1u, 0u,
        )
        cache.merge(
            profile, makeBatch("profile", true, listOf(changed("alice", 2u, 0xCC.toByte()))), 1u, 1u,
        )
        assertNull(cache.profileBytes("ghost"))
        assertArrayEquals(byteArrayOf(0x01, 0xCC.toByte()), cache.profileBytes("alice"))
        assertFalse(cache.needsResync)
    }

    @Test
    fun clearedRemovesRow() {
        val cache = KeyedRefCache()
        cache.merge(profile, makeBatch("profile", true, listOf(changed("alice", 1u, 0xAA.toByte()))), 1u, 0u)
        val changedKeys = cache.merge(profile, makeBatch("profile", false, listOf(cleared("alice", 2u))), 1u, 0u)
        assertEquals(setOf("alice"), changedKeys)
        assertNull(cache.profileBytes("alice"))
    }

    @Test
    fun staleRevIsSkipped() {
        val cache = KeyedRefCache()
        cache.merge(profile, makeBatch("profile", true, listOf(changed("alice", 5u, 0x55))), 1u, 0u)
        val changedKeys = cache.merge(profile, makeBatch("profile", false, listOf(changed("alice", 3u, 0x33))), 1u, 0u)
        assertTrue(changedKeys.isEmpty())
        assertArrayEquals(byteArrayOf(0x01, 0x55), cache.profileBytes("alice"))
    }

    // Invariant #4: typed per namespace.
    @Test
    fun namespaceIsolation() {
        val cache = KeyedRefCache()
        cache.merge(profile, makeBatch("profile", true, listOf(changed("shared", 1u, 0x11))), 1u, 0u)
        cache.merge(event, makeBatch("event", true, listOf(changed("shared", 1u, 0x22))), 1u, 0u)
        assertArrayEquals(byteArrayOf(0x01, 0x11), cache.profileBytes("shared"))
        assertArrayEquals(byteArrayOf(0x01, 0x22), cache.eventBytes("shared"))

        cache.merge(profile, makeBatch("profile", false, listOf(cleared("shared", 2u))), 1u, 0u)
        assertNull(cache.profileBytes("shared"))
        assertArrayEquals(byteArrayOf(0x01, 0x22), cache.eventBytes("shared"))
    }

    // Per-row observable: exactly one key notified.
    @Test
    fun rowChangeListenerFiresPerChangedKey() {
        val cache = KeyedRefCache()
        cache.merge(
            profile,
            makeBatch("profile", true, listOf(changed("alice", 1u, 0xAA.toByte()), changed("bob", 1u, 0xBB.toByte()))),
            1u, 0u,
        )
        val observed = mutableListOf<String>()
        cache.addRowChangeListener { change -> observed.add(change.rowKey) }
        cache.merge(profile, makeBatch("profile", false, listOf(changed("alice", 2u, 0xCC.toByte()))), 1u, 0u)
        assertEquals(listOf("alice"), observed)
    }
}
