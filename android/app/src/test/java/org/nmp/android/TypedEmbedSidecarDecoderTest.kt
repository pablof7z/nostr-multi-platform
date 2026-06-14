package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.content.ContentTreeWire as FbContentTreeWire
import nmp.content.PlaceholderReason
import nmp.content.RenderMode
import nmp.content.WireNode
import nmp.content.WireNodeKind
import nmp.embed.ArticleProjection
import nmp.embed.ClaimedEventEmbeds
import nmp.embed.EmbeddedEventEnvelope
import nmp.embed.EmbedKindProjection
import nmp.embed.EmbedProjectionKind
import nmp.embed.HighlightProjection
import nmp.embed.ProfileProjection
import nmp.embed.ShortNoteProjection
import nmp.embed.TagRow
import nmp.embed.UnknownProjection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Round-trip contract tests for [TypedEmbedSidecarDecoder] — the typed-first
 * decode of the `claimed_event_embeds` (`NEMB` / `nmp.embed.ClaimedEventEmbeds`)
 * sidecar (#1283 / #1335 item 2).
 *
 * Each test encodes a NEMB buffer via the generated Kotlin FlatBuffers builders
 * (flatc 25.2.10 bindings in `nmp/embed/`), passes it through
 * [TypedEmbedSidecarDecoder.decode], and asserts the resulting Kotlin domain
 * model matches what was encoded. Pattern mirrors [TypedSignerStateDecoderTest]
 * and [TypedAccountsDecoderTest].
 *
 * Coverage:
 *  - absent sidecar / wrong schema → empty map;
 *  - wrong file identifier → empty map (D1 fail closed);
 *  - ShortNote round-trip including `has_*` optional fields + absent display name;
 *  - Article round-trip with title/summary;
 *  - Highlight round-trip with optional source fields;
 *  - Profile round-trip;
 *  - Unknown fallback with tag rows and altText;
 *  - Multiple entries keyed by primaryId;
 *  - NFCT content-tree sub-buffer round-trips to non-empty plain text;
 *  - Collapsed envelope preserves collapseReason.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedEmbedSidecarDecoderTest {

    // ── Absent / malformed ────────────────────────────────────────────────────

    @Test
    fun absentSidecarReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(emptyList()))
    }

    @Test
    fun emptyPayloadReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(ByteArray(0)))
    }

    @Test
    fun wrongSchemaIdIsIgnored() {
        val env = TypedProjectionEnvelope(
            key = TypedEmbedSidecarDecoder.PROJECTION_KEY,
            schemaId = "wrong.schema",
            schemaVersion = 1u,
            fileIdentifier = TypedEmbedSidecarDecoder.FILE_IDENTIFIER,
            payload = emptyNembBuffer(),
        )
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(listOf(env)))
    }

    @Test
    fun wrongFileIdentifierReturnsEmptyMap() {
        val garbled = emptyNembBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber the NEMB file identifier
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(garbled))
    }

    @Test
    fun emptyEntriesVectorReturnsEmptyMap() {
        assertEquals(emptyMap<String, Any>(), TypedEmbedSidecarDecoder.decode(emptyNembBuffer()))
    }

    // ── ShortNote round-trip ─────────────────────────────────────────────────

    @Test
    fun shortNoteRoundTrip() {
        val id = "aa".repeat(32)
        val author = "bb".repeat(32)
        val buf = shortNoteNembBuffer(
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
        val buf = shortNoteNembBuffer(
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

    // ── Article round-trip ────────────────────────────────────────────────────

    @Test
    fun articleRoundTrip() {
        val primaryId = "ee".repeat(32)
        val buf = articleNembBuffer(primaryId)

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

    // ── Highlight round-trip ──────────────────────────────────────────────────

    @Test
    fun highlightRoundTrip() {
        val primaryId = "11".repeat(32)
        val buf = highlightNembBuffer(primaryId)

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

    // ── Profile round-trip ────────────────────────────────────────────────────

    @Test
    fun profileRoundTrip() {
        val primaryId = "44".repeat(32)
        val buf = profileNembBuffer(primaryId)

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

    // ── Unknown fallback round-trip ────────────────────────────────────────────

    @Test
    fun unknownRoundTrip() {
        val primaryId = "55".repeat(32)
        val buf = unknownNembBuffer(primaryId)

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

    // ── Multiple entries ──────────────────────────────────────────────────────

    @Test
    fun multipleEntriesDecodeToMapKeyedByPrimaryId() {
        // Entries are sorted ascending by primaryId for the FlatBuffers key vector.
        val id1 = "aa".repeat(32) // "aa…" < "ee…"
        val id2 = "ee".repeat(32)
        // Build both envelopes in the SAME FlatBufferBuilder, then wrap them.
        val b = FlatBufferBuilder(1024)
        val env1 = buildShortNoteEnvelope(id1, id1, "bb".repeat(32), b = b)
        val env2 = buildArticleEnvelope(id2, b = b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env1, env2))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        val buf = b.sizedByteArray()

        val map = TypedEmbedSidecarDecoder.decode(buf)

        assertEquals(2, map.size)
        assertNotNull(map[id1]?.projection?.shortNote)
        assertNotNull(map[id2]?.projection?.article)
    }

    // ── Collapse fields ───────────────────────────────────────────────────────

    @Test
    fun collapsedEnvelopePreservesCollapseReason() {
        val primaryId = "77".repeat(32)
        val buf = collapsedNembBuffer(primaryId, reason = "dangling")

        val entry = requireNotNull(TypedEmbedSidecarDecoder.decode(buf)[primaryId])
        assertTrue(entry.collapsed)
        assertEquals("dangling", entry.collapseReason)
        // Projection still decoded — caller decides render suppression (D0).
        assertNotNull(entry.projection?.shortNote)
    }

    // ── NFCT content-tree sub-buffer ─────────────────────────────────────────

    @Test
    fun contentTreeSubBufferDecodesToNonEmptyPlainText() {
        val primaryId = "88".repeat(32)
        val nfct = buildTextNfctBuffer("Hello embed world")
        val buf = shortNoteNembBuffer(
            primaryId = primaryId,
            id = primaryId,
            authorPubkey = "99".repeat(32),
            nfctBytes = nfct,
        )
        val note = requireNotNull(TypedEmbedSidecarDecoder.decode(buf)[primaryId]?.projection?.shortNote)
        assertEquals("Hello embed world", note.content)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Buffer builders — each function returns a complete NEMB ByteArray
    // ─────────────────────────────────────────────────────────────────────────

    private fun emptyNembBuffer(): ByteArray {
        val b = FlatBufferBuilder(64)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf())
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    /** Single-entry NEMB buffer with a ShortNote projection. */
    private fun shortNoteNembBuffer(
        primaryId: String,
        id: String = primaryId,
        authorPubkey: String = "cc".repeat(32),
        hasAuthorDisplayName: Boolean = false,
        authorDisplayName: String? = null,
        hasAuthorPictureUrl: Boolean = false,
        authorPictureUrl: String? = null,
        createdAt: ULong = 0UL,
        mediaUrls: List<String> = emptyList(),
        nfctBytes: ByteArray = ByteArray(0),
    ): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildShortNoteEnvelope(
            primaryId, id, authorPubkey,
            hasAuthorDisplayName, authorDisplayName,
            hasAuthorPictureUrl, authorPictureUrl,
            createdAt, mediaUrls, nfctBytes, b,
        )
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    private fun articleNembBuffer(primaryId: String): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildArticleEnvelope(primaryId, b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    private fun highlightNembBuffer(primaryId: String): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildHighlightEnvelope(primaryId, b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    private fun profileNembBuffer(primaryId: String): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildProfileEnvelope(primaryId, b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    private fun unknownNembBuffer(primaryId: String): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildUnknownEnvelope(primaryId, b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    private fun collapsedNembBuffer(primaryId: String, reason: String): ByteArray {
        val b = FlatBufferBuilder(512)
        val env = buildCollapsedEnvelope(primaryId, reason, b)
        val entries = ClaimedEventEmbeds.createEntriesVector(b, intArrayOf(env))
        val root = ClaimedEventEmbeds.createClaimedEventEmbeds(b, entries)
        ClaimedEventEmbeds.finishClaimedEventEmbedsBuffer(b, root)
        return b.sizedByteArray()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Envelope builders — each returns an offset into the given FlatBufferBuilder
    // ─────────────────────────────────────────────────────────────────────────

    private fun buildShortNoteEnvelope(
        primaryId: String,
        id: String,
        authorPubkey: String,
        hasAuthorDisplayName: Boolean = false,
        authorDisplayName: String? = null,
        hasAuthorPictureUrl: Boolean = false,
        authorPictureUrl: String? = null,
        createdAt: ULong = 0UL,
        mediaUrls: List<String> = emptyList(),
        nfctBytes: ByteArray = ByteArray(0),
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val idOff = b.createString(id)
        val pkOff = b.createString(authorPubkey)
        val dnOff = if (authorDisplayName != null) b.createString(authorDisplayName) else 0
        val puOff = if (authorPictureUrl != null) b.createString(authorPictureUrl) else 0
        val mediaOff = if (mediaUrls.isNotEmpty()) {
            ShortNoteProjection.createMediaUrlsVector(b, mediaUrls.map { b.createString(it) }.toIntArray())
        } else 0
        val nfctOff = if (nfctBytes.isNotEmpty()) {
            ShortNoteProjection.createContentTreeVector(b, nfctBytes.toUByteArray())
        } else 0
        val note = ShortNoteProjection.createShortNoteProjection(
            b, idOff, pkOff, hasAuthorDisplayName, dnOff,
            hasAuthorPictureUrl, puOff, createdAt, nfctOff, mediaOff,
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.ShortNote, note, 0, 0, 0, 0,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u, false, false, 0, kindProj,
        )
    }

    private fun buildArticleEnvelope(
        primaryId: String,
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val idOff = b.createString(primaryId)
        val pkOff = b.createString("ff".repeat(32))
        val dnOff = b.createString("Bob")
        val dTagOff = b.createString("test-d-tag")
        val titleOff = b.createString("Test Article")
        val summaryOff = b.createString("A summary")
        val article = ArticleProjection.createArticleProjection(
            b,
            idOff, pkOff,
            true, dnOff,      // hasAuthorDisplayName, authorDisplayName
            false, 0,         // hasAuthorPictureUrl, authorPictureUrl
            1_700_000_001UL,
            true, titleOff,   // hasTitle, title
            true, summaryOff, // hasSummary, summary
            false, 0,         // hasHeroImageUrl, heroImageUrl
            dTagOff, 0,       // dTag, contentTree (empty)
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.Article, 0, article, 0, 0, 0,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u, false, false, 0, kindProj,
        )
    }

    private fun buildHighlightEnvelope(
        primaryId: String,
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val idOff = b.createString(primaryId)
        val pkOff = b.createString("22".repeat(32))
        val textOff = b.createString("Highlighted text here")
        val srcEventOff = b.createString("33".repeat(32))
        val hl = HighlightProjection.createHighlightProjection(
            b,
            idOff, pkOff,
            false, 0,        // hasAuthorDisplayName, authorDisplayName
            0UL,             // createdAt
            textOff,
            true, srcEventOff, // hasSourceEventId, sourceEventId
            false, 0,        // hasSourceEventAddr, sourceEventAddr
            false, 0,        // hasSourceUrl, sourceUrl
            false, 0,        // hasContext, context
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.Highlight, 0, 0, hl, 0, 0,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u, false, false, 0, kindProj,
        )
    }

    private fun buildProfileEnvelope(
        primaryId: String,
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val pkOff = b.createString(primaryId)
        val dnOff = b.createString("Carol")
        val picOff = b.createString("https://example.com/pic.jpg")
        val prof = ProfileProjection.createProfileProjection(
            b, pkOff,
            true, dnOff,   // hasDisplayName, displayName
            true, picOff,  // hasPictureUrl, pictureUrl
            false, 0,      // hasAbout, about
            false, 0,      // hasNip05, nip05
            false, 0,      // hasLud16, lud16
            false, 0,      // hasBannerUrl, bannerUrl
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.Profile, 0, 0, 0, prof, 0,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u, false, false, 0, kindProj,
        )
    }

    private fun buildUnknownEnvelope(
        primaryId: String,
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val pkOff = b.createString("66".repeat(32))
        val contentOff = b.createString("raw content")
        val altOff = b.createString("alt description")
        // Build one tag row: ["e", "eventid", "wss://relay.example.com"]
        val v0 = b.createString("e")
        val v1 = b.createString("eventid")
        val v2 = b.createString("wss://relay.example.com")
        val valVec = TagRow.createValuesVector(b, intArrayOf(v0, v1, v2))
        val tagRow = TagRow.createTagRow(b, valVec)
        val tagsVec = UnknownProjection.createTagsVector(b, intArrayOf(tagRow))
        val unk = UnknownProjection.createUnknownProjection(
            b, 9999u, pkOff,
            false, 0,      // hasAuthorDisplayName, authorDisplayName
            false, 0,      // hasAuthorPictureUrl, authorPictureUrl
            0UL,           // createdAt
            contentOff, 0, // content, contentTree (empty)
            tagsVec,
            true, altOff,  // hasAltText, altText
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.Unknown, 0, 0, 0, 0, unk,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u, false, false, 0, kindProj,
        )
    }

    private fun buildCollapsedEnvelope(
        primaryId: String,
        reason: String,
        b: FlatBufferBuilder = FlatBufferBuilder(512),
    ): Int {
        val pidOff = b.createString(primaryId)
        val uriOff = b.createString("")
        val reasonOff = b.createString(reason)
        // Minimal ShortNote stub so the decoder can map the projection.
        val idOff = b.createString(primaryId)
        val pkOff = b.createString("aa".repeat(32))
        val note = ShortNoteProjection.createShortNoteProjection(
            b, idOff, pkOff, false, 0, false, 0, 0UL, 0, 0,
        )
        val kindProj = EmbedKindProjection.createEmbedKindProjection(
            b, EmbedProjectionKind.ShortNote, note, 0, 0, 0, 0,
        )
        return EmbeddedEventEnvelope.createEmbeddedEventEnvelope(
            b, pidOff, uriOff, 0u, 4u,
            true,       // collapsed
            true,       // hasCollapseReason
            reasonOff, kindProj,
        )
    }

    // ── NFCT fixture builder ──────────────────────────────────────────────────

    /**
     * Build a minimal `ContentTreeWire` (`NFCT`) buffer carrying a single text
     * node so [TypedEmbedSidecarDecoder]'s NFCT sub-buffer path can be tested.
     */
    private fun buildTextNfctBuffer(text: String): ByteArray {
        val b = FlatBufferBuilder(256)
        val textStr = b.createString(text)
        val node = WireNode.createWireNode(
            b,
            WireNodeKind.Text,
            textStr, // text slot
            0, 0, 0, 0, 0, 0u, 0, 0, 0u, 0,
            -1L,     // orderedStart default sentinel
            0, 0, 0, 0,
            PlaceholderReason.DepthLimit,
            0u, 0,
        )
        val nodesVec = FbContentTreeWire.createNodesVector(b, intArrayOf(node))
        val rootsVec = FbContentTreeWire.createRootsVector(b, uintArrayOf(0u))
        val tree = FbContentTreeWire.createContentTreeWire(b, nodesVec, rootsVec, RenderMode.Auto)
        FbContentTreeWire.finishContentTreeWireBuffer(b, tree)
        return b.sizedByteArray()
    }
}
