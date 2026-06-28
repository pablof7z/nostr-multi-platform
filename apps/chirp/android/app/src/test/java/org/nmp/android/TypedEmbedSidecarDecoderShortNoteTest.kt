package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * ShortNote-family round-trip contract tests for [TypedEmbedSidecarDecoder] —
 * the typed-first decode of the `refs.event.envelopes` (`NEMB` /
 * `nmp.embed.RefEventEnvelopes`) sidecar (#1283 / #1335 item 2).
 *
 * Covers the ShortNote projection plus the envelope-level concerns that ride on
 * it: optional `has_*` fields, multiple entries keyed by primaryId, the NFCT
 * content-tree sub-buffer, and the collapsed envelope. Shared FlatBuffers
 * fixtures live in [NembTestFixtures]; the Article/Highlight/Profile/Unknown
 * kinds and the error paths live in sibling test files.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedEmbedSidecarDecoderShortNoteTest {

    @Test
    fun shortNoteRoundTrip() {
        val id = "aa".repeat(32)
        val author = "bb".repeat(32)
        val buf = NembTestFixtures.shortNoteNembBuffer(
            primaryId = id,
            id = id,
            authorPubkey = author,
            hasAuthorDisplayName = true,
            authorDisplayName = "Alice",
            hasAuthorPictureUrl = false,
            createdAt = 1_700_000_000UL,
            mediaUrls = listOf("https://example.com/img.png"),
        )

        val map = TypedEmbedSidecarDecoder.decode(buf)
        assertEquals(1, map.size)

        val entry = requireNotNull(map[id]) { "entry must be keyed by primaryId" }
        assertEquals(id, entry.primaryId)
        assertFalse(entry.collapsed)
        assertNull(entry.collapseReason)

        val note = requireNotNull(entry.projection?.shortNote) { "shortNote must be set" }
        assertNull(entry.projection?.article)
        assertNull(entry.projection?.highlight)
        assertNull(entry.projection?.profile)
        assertNull(entry.projection?.unknown)

        assertEquals(id, note.id)
        assertEquals(author, note.authorPubkey)
        assertEquals("Alice", note.authorDisplayName)
        assertNull(note.authorPictureUrl)
        assertEquals(1_700_000_000L, note.createdAt)
        assertEquals(listOf("https://example.com/img.png"), note.mediaUrls)
    }

    @Test
    fun shortNoteAbsentDisplayNameIsNull() {
        val id = "cc".repeat(32)
        val buf = NembTestFixtures.shortNoteNembBuffer(
            primaryId = id,
            id = id,
            authorPubkey = "dd".repeat(32),
            hasAuthorDisplayName = false,
            hasAuthorPictureUrl = false,
        )
        val note = requireNotNull(TypedEmbedSidecarDecoder.decode(buf)[id]?.projection?.shortNote)
        assertNull(note.authorDisplayName)
        assertNull(note.authorPictureUrl)
    }

    @Test
    fun multipleEntriesDecodeToMapKeyedByPrimaryId() {
        // Entries are sorted ascending by primaryId for the FlatBuffers key vector.
        val id1 = "aa".repeat(32) // "aa…" < "ee…"
        val id2 = "ee".repeat(32)
        // Build both envelopes in the SAME FlatBufferBuilder, then wrap them.
        val b = FlatBufferBuilder(1024)
        val env1 = NembTestFixtures.buildShortNoteEnvelope(id1, id1, "bb".repeat(32), b = b)
        val env2 = NembTestFixtures.buildArticleEnvelope(id2, b = b)
        val buf = NembTestFixtures.wrapEntries(b, intArrayOf(env1, env2))

        val map = TypedEmbedSidecarDecoder.decode(buf)

        assertEquals(2, map.size)
        assertNotNull(map[id1]?.projection?.shortNote)
        assertNotNull(map[id2]?.projection?.article)
    }

    @Test
    fun collapsedEnvelopePreservesCollapseReason() {
        val primaryId = "77".repeat(32)
        val buf = NembTestFixtures.collapsedNembBuffer(primaryId, reason = "dangling")

        val entry = requireNotNull(TypedEmbedSidecarDecoder.decode(buf)[primaryId])
        assertTrue(entry.collapsed)
        assertEquals("dangling", entry.collapseReason)
        // Projection still decoded — caller decides render suppression (D0).
        assertNotNull(entry.projection?.shortNote)
    }

    @Test
    fun contentTreeSubBufferDecodesToNonEmptyPlainText() {
        val primaryId = "88".repeat(32)
        val nfct = NembTestFixtures.buildTextNfctBuffer("Hello embed world")
        val buf = NembTestFixtures.shortNoteNembBuffer(
            primaryId = primaryId,
            id = primaryId,
            authorPubkey = "99".repeat(32),
            nfctBytes = nfct,
        )
        val note = requireNotNull(TypedEmbedSidecarDecoder.decode(buf)[primaryId]?.projection?.shortNote)
        assertEquals("Hello embed world", note.content)
    }
}
