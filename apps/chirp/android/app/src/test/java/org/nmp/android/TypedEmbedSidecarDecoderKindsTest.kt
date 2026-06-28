package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Article / Highlight / Profile / Unknown round-trip contract tests for
 * [TypedEmbedSidecarDecoder] — the typed-first decode of the
 * `refs.event.envelopes` (`NEMB` / `nmp.embed.RefEventEnvelopes`) sidecar
 * (#1283 / #1335 item 2).
 *
 * Covers the non-ShortNote `EmbedProjectionKind` variants. Shared FlatBuffers
 * fixtures live in [NembTestFixtures]; ShortNote + envelope-level concerns and
 * the error paths live in sibling test files.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedEmbedSidecarDecoderKindsTest {

    @Test
    fun articleRoundTrip() {
        val primaryId = "ee".repeat(32)
        val buf = NembTestFixtures.articleNembBuffer(primaryId)

        val map = TypedEmbedSidecarDecoder.decode(buf)
        assertEquals(1, map.size)

        val entry = requireNotNull(map[primaryId])
        assertNull(entry.projection?.shortNote)
        val article = requireNotNull(entry.projection?.article) { "article must be set" }

        assertEquals(primaryId, article.id)
        assertEquals("ff".repeat(32), article.authorPubkey)
        assertEquals("Bob", article.authorDisplayName)
        assertNull(article.authorPictureUrl)
        assertEquals("Test Article", article.title)
        assertEquals("A summary", article.summary)
        assertNull(article.heroImageUrl)
        assertEquals("test-d-tag", article.dTag)
        assertEquals(1_700_000_001L, article.createdAt)
    }

    @Test
    fun highlightRoundTrip() {
        val primaryId = "11".repeat(32)
        val buf = NembTestFixtures.highlightNembBuffer(primaryId)

        val map = TypedEmbedSidecarDecoder.decode(buf)
        assertEquals(1, map.size)

        val hl = requireNotNull(map[primaryId]?.projection?.highlight)
        assertEquals(primaryId, hl.id)
        assertEquals("22".repeat(32), hl.authorPubkey)
        assertEquals("Highlighted text here", hl.highlightedText)
        assertEquals("33".repeat(32), hl.sourceEventId)
        assertNull(hl.sourceEventAddr)
        assertNull(hl.sourceUrl)
        assertNull(hl.context)
    }

    @Test
    fun profileRoundTrip() {
        val primaryId = "44".repeat(32)
        val buf = NembTestFixtures.profileNembBuffer(primaryId)

        val map = TypedEmbedSidecarDecoder.decode(buf)
        assertEquals(1, map.size)

        val prof = requireNotNull(map[primaryId]?.projection?.profile)
        assertEquals(primaryId, prof.pubkey)
        assertEquals("Carol", prof.displayName)
        assertEquals("https://example.com/pic.jpg", prof.pictureUrl)
        assertNull(prof.about)
        assertNull(prof.nip05)
        assertNull(prof.lud16)
        assertNull(prof.bannerUrl)
    }

    @Test
    fun unknownRoundTrip() {
        val primaryId = "55".repeat(32)
        val buf = NembTestFixtures.unknownNembBuffer(primaryId)

        val map = TypedEmbedSidecarDecoder.decode(buf)
        assertEquals(1, map.size)

        val unk = requireNotNull(map[primaryId]?.projection?.unknown)
        assertEquals(9999, unk.kind)
        assertEquals("66".repeat(32), unk.authorPubkey)
        assertEquals("raw content", unk.content)
        assertEquals(1, unk.tags.size)
        assertEquals(listOf("e", "eventid", "wss://relay.example.com"), unk.tags[0])
        assertEquals("alt description", unk.altText)
    }
}
