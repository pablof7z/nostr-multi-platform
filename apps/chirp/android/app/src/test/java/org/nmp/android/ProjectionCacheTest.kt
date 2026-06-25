package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.AccountsSnapshot
import nmp.kernel.ActiveAccountSnapshot
import nmp.kernel.AccountSummaryRow
import nmp.transport.FrameKind
import nmp.transport.Metrics
import nmp.transport.ProjectionPresenceState
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Unit tests for [ProjectionMergeCache] (ADR-0055 R3-S4 / D3-3).
 *
 * Each test builds real FlatBuffers frames using the same helpers as
 * [TypedAccountsDecoderTest] so the behavioural invariants are verified
 * against actual serialised bytes — not hand-crafted [TypedProjectionEnvelope]
 * literals.
 *
 * Coverage mirrors [apps/chirp/ios/ChirpTests/ProjectionCacheTests.swift]:
 *  1. Omitted key retains prior cached value (D3-3 Unchanged case)
 *  2. Cleared key is removed from cache (D3-3 Cleared case)
 *  3. Changed key with higher rev overwrites cache (D3-3 Changed case)
 *  4. Reorder guard: stale rev is silently skipped (D3 belt-and-braces)
 *  5. Session or epoch change triggers full cache reset (D4) then re-baselines
 *  6. decode-before-commit: empty payload keeps prior + latches needsResync (D3-4)
 *  7. session_id == 0: pass-through without trusting omission (D3-5)
 *  8. changedKeys exact set (Changed + Cleared only)
 *  9. baselined flips to true after first post-baseline frame
 * 10. Cleared key appears in changedKeys
 * 11. Multi-key frame: independent key processing
 * 12. reset() returns cache to clean state
 */
@OptIn(ExperimentalUnsignedTypes::class)
class ProjectionCacheTest {

    private lateinit var cache: ProjectionMergeCache

    @Before
    fun setUp() {
        cache = ProjectionMergeCache()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. Omitted key retains prior cached value
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun omittedKeyRetainsPriorCachedValue() {
        val bytes = accountsBuffer("alice")
        // Frame 1: populate cache with "accounts" key.
        val env1 = changedEnvelope("accounts", "accounts", bytes, rev = 1UL)
        val result1 = cache.merge(listOf(env1), sessionId = 1UL, snapshotEpoch = 1UL)
        assertEquals(1, result1.mergedEnvelopes.size)
        assertEquals("accounts", result1.mergedEnvelopes[0].key)

        // Frame 2: no "accounts" key in incoming envelopes (omitted / unchanged).
        val result2 = cache.merge(emptyList(), sessionId = 1UL, snapshotEpoch = 1UL)
        // The prior cache entry must be present in the merged set.
        assertEquals(1, result2.mergedEnvelopes.size)
        assertEquals("accounts", result2.mergedEnvelopes[0].key)
        assertTrue(result2.mergedEnvelopes[0].payload.contentEquals(bytes))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. Cleared key is removed from cache
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun clearedKeyIsRemovedFromCache() {
        val bytes = accountsBuffer("alice")
        // Seed the cache.
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Cleared row for "accounts".
        val cleared = clearedEnvelope("accounts", rev = 2UL)
        val result = cache.merge(listOf(cleared), sessionId = 1UL, snapshotEpoch = 1UL)

        // Cleared key must be absent from the merged set.
        assertTrue("Cleared key must be absent from mergedEnvelopes",
            result.mergedEnvelopes.none { it.key == "accounts" })
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Changed key with higher rev overwrites cache
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun changedKeyWithHigherRevOverwritesCache() {
        val bytesV1 = accountsBuffer("alice")
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytesV1, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        val bytesV2 = accountsBuffer("bob")
        val result = cache.merge(
            listOf(changedEnvelope("accounts", "accounts", bytesV2, rev = 2UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        val found = result.mergedEnvelopes.firstOrNull { it.key == "accounts" }
        assertNotNull("Changed key must be present in merged set", found)
        assertTrue("Cache must reflect the updated payload",
            found!!.payload.contentEquals(bytesV2))
        assertEquals(2UL, found.projectionRev)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. Reorder guard: stale rev is silently skipped
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun staleRevIsSkippedAndPriorCacheEntryRetained() {
        val bytesV2 = accountsBuffer("alice")
        // Write rev=2 to cache.
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytesV2, rev = 2UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Attempt to write rev=1 (stale).
        val bytesV1 = accountsBuffer("stale")
        val result = cache.merge(
            listOf(changedEnvelope("accounts", "accounts", bytesV1, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        val found = result.mergedEnvelopes.firstOrNull { it.key == "accounts" }
        assertNotNull(found)
        assertTrue("Stale rev must NOT overwrite the cache; prior payload (rev=2) must be retained",
            found!!.payload.contentEquals(bytesV2))
        assertEquals(2UL, found.projectionRev)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. Session or epoch change triggers full cache reset (D4), then re-baselines
    //    The reset is ATOMIC (before the row loop), so the incoming frame's rows
    //    are applied to the freshly-cleared cache.
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun sessionChangeResetsCache() {
        val bytes = accountsBuffer("alice")
        // Populate session 1.
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)
        assertTrue("Cache must be baselined after first frame", cache.baselined)

        // New session: same frame content but sessionId changed.
        val newBytes = accountsBuffer("bob")
        val result = cache.merge(
            listOf(changedEnvelope("accounts", "accounts", newBytes, rev = 1UL)),
            sessionId = 2UL, snapshotEpoch = 1UL)

        // After reset + re-baseline, the merged set reflects the new frame only.
        val found = result.mergedEnvelopes.firstOrNull { it.key == "accounts" }
        assertNotNull(found)
        assertTrue("Cache must reflect the post-reset frame payload",
            found!!.payload.contentEquals(newBytes))
        assertTrue("Cache must be baselined after post-reset frame", cache.baselined)
    }

    @Test
    fun epochChangeResetsCache() {
        val bytes = accountsBuffer("alice")
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Epoch advanced — cache must reset.
        val result = cache.merge(emptyList(), sessionId = 1UL, snapshotEpoch = 2UL)
        // No rows in frame → merged set is empty (cache was cleared by D4 reset).
        assertTrue("Cache must be empty after epoch-triggered reset with no rows",
            result.mergedEnvelopes.isEmpty())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. decode-before-commit: empty payload keeps prior + latches needsResync (D3-4)
    //    The canonical decode-failure the cache defends against is a Changed row
    //    carrying EMPTY payload bytes (a malformed frame).
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun emptyPayloadKeepsPriorCacheEntryAndLatchesNeedsResync() {
        val goodBytes = accountsBuffer("alice")
        // Seed the cache with a good entry.
        cache.merge(listOf(changedEnvelope("accounts", "accounts", goodBytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)
        assertFalse("needsResync must be clear before decode failure", cache.needsResync)

        // Incoming Changed row with empty payload — malformed frame.
        val emptyPayload = TypedProjectionEnvelope(
            key = "accounts",
            schemaId = "accounts",
            schemaVersion = 1u,
            fileIdentifier = "KACC",
            payload = ByteArray(0), // EMPTY — decode failure trigger
            projectionRev = 2UL,
            state = ProjectionPresenceState.Changed,
        )
        val result = cache.merge(listOf(emptyPayload), sessionId = 1UL, snapshotEpoch = 1UL)

        // Prior cache entry must be retained (no silent corruption).
        val found = result.mergedEnvelopes.firstOrNull { it.key == "accounts" }
        assertNotNull("Prior cache entry must be retained on decode failure", found)
        assertTrue("Prior payload must be intact",
            found!!.payload.contentEquals(goodBytes))
        // needsResync must be latched.
        assertTrue("needsResync must be latched after decode failure", cache.needsResync)
        assertTrue("MergeResult.needsResync must also be set", result.needsResync)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6b. NON-EMPTY corrupt payload (mirrors iOS test 12 corrupt vector).
    //     A Changed row carrying non-empty garbage bytes (byteArrayOf(0x00))
    //     PASSES the isNotEmpty() decode-before-commit gate, so it commits to the
    //     cache. The documented fail-closed contract is then end-to-end through
    //     decodeProjections: the typed decoder rejects the corrupt buffer
    //     (BufferHasIdentifier check + try/catch) and the slot defaults — NO
    //     crash. The cache self-heals when a subsequent valid Changed row at a
    //     higher rev arrives. This pins the corrupt-payload contract.
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun nonEmptyCorruptPayloadFailsClosedThenSelfHeals() {
        val goodBytes = accountsBuffer("alice")
        // Seed the cache with a valid entry (rev 1).
        cache.merge(listOf(changedEnvelope("accounts", "accounts", goodBytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Sanity: the seeded value decodes end-to-end via decodeProjections.
        val seeded = cache.merge(emptyList(), sessionId = 1UL, snapshotEpoch = 1UL)
        val seededProjections = KernelUpdateFrameDecoder.decodeProjections(seeded.mergedEnvelopes)
        assertEquals("seeded accounts must decode to one row", 1, seededProjections.accounts.size)

        // Frame 2: a NON-EMPTY corrupt payload as a Changed row (rev 2).
        // 0x00 is non-empty so it passes the decode-before-commit gate and commits.
        val corrupt = TypedProjectionEnvelope(
            key = "accounts",
            schemaId = "accounts",
            schemaVersion = 1u,
            fileIdentifier = "KACC",
            payload = byteArrayOf(0x00), // non-empty garbage
            projectionRev = 2UL,
            state = ProjectionPresenceState.Changed,
        )
        val corruptResult = cache.merge(listOf(corrupt), sessionId = 1UL, snapshotEpoch = 1UL)

        // Fail-closed end-to-end: decodeProjections does NOT crash, and the
        // corrupt accounts buffer (no KACC identifier) yields an EMPTY accounts
        // list (the typed decoder fails closed; the slot defaults).
        val corruptProjections = KernelUpdateFrameDecoder.decodeProjections(corruptResult.mergedEnvelopes)
        assertTrue("corrupt accounts buffer must default the slot (fail-closed, no crash)",
            corruptProjections.accounts.isEmpty())

        // Frame 3: a VALID Changed row at a higher rev (rev 3) — the cache self-heals.
        val healedBytes = accountsBuffer("healed")
        val healResult = cache.merge(
            listOf(changedEnvelope("accounts", "accounts", healedBytes, rev = 3UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)
        val healedProjections = KernelUpdateFrameDecoder.decodeProjections(healResult.mergedEnvelopes)
        assertEquals("cache must self-heal to a valid row after a higher-rev Changed frame",
            1, healedProjections.accounts.size)
        assertEquals("healed", healedProjections.accounts[0].displayName)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. session_id == 0: pass-through without trusting omission (D3-5)
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun sessionIdZeroPassesThroughWithoutTrustingOmission() {
        val bytes = accountsBuffer("alice")
        // Seed the cache under a real session first.
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Frame with sessionId == 0 (no incremental contract).
        val freshBytes = accountsBuffer("direct")
        val incoming = listOf(changedEnvelope("accounts", "accounts", freshBytes, rev = 99UL))
        val result = cache.merge(incoming, sessionId = 0UL, snapshotEpoch = 0UL)

        // Pass-through: the envelopes arrive as-is (not merged from cache).
        assertEquals("session_id==0 must pass envelopes through unchanged", 1, result.mergedEnvelopes.size)
        assertTrue("Pass-through payload must be the raw incoming bytes",
            result.mergedEnvelopes[0].payload.contentEquals(freshBytes))
        // changedKeys is conservative (all keys from the frame).
        assertTrue("accounts must be in changedKeys under session_id==0",
            result.changedKeys.contains("accounts"))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. changedKeys exact set (Changed + Cleared only; omitted NOT included)
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun changedKeysExactSet() {
        val bytes = accountsBuffer("alice")
        val activeBytes = activeAccountBuffer("pk1")
        // Seed both keys.
        cache.merge(
            listOf(
                changedEnvelope("accounts", "accounts", bytes, rev = 1UL),
                changedEnvelope("active_account", "active_account", activeBytes, rev = 1UL),
            ),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Frame: only "active_account" changed; "accounts" omitted.
        val newActiveBytes = activeAccountBuffer("pk2")
        val result = cache.merge(
            listOf(changedEnvelope("active_account", "active_account", newActiveBytes, rev = 2UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // changedKeys must contain only the key that was in the frame.
        assertEquals("changedKeys must be exactly {active_account}",
            setOf("active_account"), result.changedKeys)
        assertFalse("omitted 'accounts' must NOT be in changedKeys",
            result.changedKeys.contains("accounts"))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 9. baselined flips to true after first post-baseline frame
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun baselinedFlipsAfterFirstFrame() {
        assertFalse("baselined must start false", cache.baselined)
        val bytes = accountsBuffer("alice")
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)
        assertTrue("baselined must be true after the first real frame", cache.baselined)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 10. Cleared key appears in changedKeys
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun clearedKeyAppearsInChangedKeys() {
        val bytes = accountsBuffer("alice")
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        val result = cache.merge(
            listOf(clearedEnvelope("accounts", rev = 2UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)

        assertTrue("Cleared key must appear in changedKeys so the caller can null-out the slot",
            result.changedKeys.contains("accounts"))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 11. Multi-key frame: independent key processing
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun multiKeyFrameProcessedIndependently() {
        val acctBytes = accountsBuffer("alice")
        val activeBytes = activeAccountBuffer("pk1")
        // Seed both.
        cache.merge(
            listOf(
                changedEnvelope("accounts", "accounts", acctBytes, rev = 1UL),
                changedEnvelope("active_account", "active_account", activeBytes, rev = 1UL),
            ),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // Frame: update "active_account" (rev 2), clear "accounts".
        val newActiveBytes = activeAccountBuffer("pk2")
        val result = cache.merge(
            listOf(
                changedEnvelope("active_account", "active_account", newActiveBytes, rev = 2UL),
                clearedEnvelope("accounts", rev = 2UL),
            ),
            sessionId = 1UL, snapshotEpoch = 1UL)

        // "active_account" must be updated.
        val active = result.mergedEnvelopes.firstOrNull { it.key == "active_account" }
        assertNotNull(active)
        assertTrue(active!!.payload.contentEquals(newActiveBytes))
        // "accounts" must be absent (Cleared).
        assertNull(result.mergedEnvelopes.firstOrNull { it.key == "accounts" })
        // Both keys in changedKeys.
        assertTrue(result.changedKeys.contains("active_account"))
        assertTrue(result.changedKeys.contains("accounts"))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 12. reset() returns cache to clean state
    // ─────────────────────────────────────────────────────────────────────────

    @Test
    fun resetClearsAllState() {
        val bytes = accountsBuffer("alice")
        cache.merge(listOf(changedEnvelope("accounts", "accounts", bytes, rev = 1UL)),
            sessionId = 1UL, snapshotEpoch = 1UL)
        assertTrue("Cache must be baselined before reset", cache.baselined)

        cache.reset()

        assertFalse("baselined must be false after reset", cache.baselined)
        assertFalse("needsResync must be false after reset", cache.needsResync)
        // After reset, first merge starts fresh — no cached values.
        val result = cache.merge(emptyList(), sessionId = 1UL, snapshotEpoch = 1UL)
        assertTrue("Merged envelopes must be empty after reset with empty frame",
            result.mergedEnvelopes.isEmpty())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Builders
    // ─────────────────────────────────────────────────────────────────────────

    /** Build a [TypedProjectionEnvelope] with state=Changed and real FlatBuffers payload. */
    private fun changedEnvelope(
        key: String,
        schemaId: String,
        payload: ByteArray,
        rev: ULong,
    ): TypedProjectionEnvelope = TypedProjectionEnvelope(
        key = key,
        schemaId = schemaId,
        schemaVersion = 1u,
        fileIdentifier = fileIdFor(key),
        payload = payload,
        projectionRev = rev,
        state = ProjectionPresenceState.Changed,
    )

    /** Build a [TypedProjectionEnvelope] with state=Cleared (tombstone). */
    private fun clearedEnvelope(key: String, rev: ULong): TypedProjectionEnvelope =
        TypedProjectionEnvelope(
            key = key,
            schemaId = "",
            schemaVersion = 0u,
            fileIdentifier = "",
            payload = ByteArray(0),
            projectionRev = rev,
            state = ProjectionPresenceState.Cleared,
        )

    /** `KACC` buffer with one named account. */
    private fun accountsBuffer(displayName: String): ByteArray {
        val b = FlatBufferBuilder(256)
        val idOff = b.createString("id-$displayName")
        val npubOff = b.createString("npub1test")
        val dnOff = b.createString(displayName)
        val skOff = b.createString("local")
        val statusOff = b.createString("active")
        // signer_label removed from the wire (#1712) — derived shell-side.
        val row = AccountSummaryRow.createAccountSummaryRow(
            b, idOff, npubOff,
            /* hasDisplayName = */ true, dnOff,
            skOff, statusOff,
            /* signerIsRemote = */ false,
            /* isActive = */ true,
            /* hasPictureUrl = */ false, /* pictureUrl = */ 0,
        )
        val vec = AccountsSnapshot.createAccountsVector(b, intArrayOf(row))
        val snap = AccountsSnapshot.createAccountsSnapshot(b, vec)
        AccountsSnapshot.finishAccountsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** `KACT` buffer; `pubkey == null` → has_active_account == false. */
    private fun activeAccountBuffer(pubkey: String?): ByteArray {
        val b = FlatBufferBuilder(128)
        val pkOff = if (pubkey != null) b.createString(pubkey) else 0
        val snap = ActiveAccountSnapshot.createActiveAccountSnapshot(b, pubkey != null, pkOff)
        ActiveAccountSnapshot.finishActiveAccountSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    private fun fileIdFor(key: String): String = when (key) {
        "accounts" -> "KACC"
        "active_account" -> "KACT"
        else -> "UNKN"
    }
}
