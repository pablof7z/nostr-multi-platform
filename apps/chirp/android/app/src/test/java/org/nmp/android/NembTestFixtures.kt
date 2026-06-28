package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.content.ContentTreeWire as FbContentTreeWire
import nmp.content.PlaceholderReason
import nmp.content.RenderMode
import nmp.content.WireNode
import nmp.content.WireNodeKind
import nmp.embed.ArticleProjection
import nmp.embed.RefEventEnvelopes
import nmp.embed.EmbeddedEventEnvelope
import nmp.embed.EmbedKindProjection
import nmp.embed.EmbedProjectionKind
import nmp.embed.HighlightProjection
import nmp.embed.ProfileProjection
import nmp.embed.ShortNoteProjection
import nmp.embed.TagRow
import nmp.embed.UnknownProjection

/**
 * Shared NEMB FlatBuffers fixture builders for the [TypedEmbedSidecarDecoder]
 * round-trip contract tests (#1283 / #1335 item 2). Each `*NembBuffer` returns a
 * complete `NEMB` ([nmp.embed.RefEventEnvelopes]) ByteArray; each
 * `build*Envelope` returns an offset into a caller-supplied [FlatBufferBuilder]
 * so multiple entries can pack into one buffer. Extracted from the original test
 * class to keep each concern-scoped file under the AGENTS.md size caps.
 */
@OptIn(ExperimentalUnsignedTypes::class)
object NembTestFixtures {

    // Whole-buffer builders — each returns a complete NEMB ByteArray.

    fun emptyNembBuffer(): ByteArray {
        val b = FlatBufferBuilder(64)
        val entries = RefEventEnvelopes.createEntriesVector(b, intArrayOf())
        val root = RefEventEnvelopes.createRefEventEnvelopes(b, entries)
        RefEventEnvelopes.finishRefEventEnvelopesBuffer(b, root)
        return b.sizedByteArray()
    }

    /** Single-entry NEMB buffer with a ShortNote projection. */
    fun shortNoteNembBuffer(
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
    ): ByteArray = wrapSingle { b ->
        buildShortNoteEnvelope(
            primaryId, id, authorPubkey,
            hasAuthorDisplayName, authorDisplayName,
            hasAuthorPictureUrl, authorPictureUrl,
            createdAt, mediaUrls, nfctBytes, b,
        )
    }

    fun articleNembBuffer(primaryId: String): ByteArray =
        wrapSingle { b -> buildArticleEnvelope(primaryId, b) }

    fun highlightNembBuffer(primaryId: String): ByteArray =
        wrapSingle { b -> buildHighlightEnvelope(primaryId, b) }

    fun profileNembBuffer(primaryId: String): ByteArray =
        wrapSingle { b -> buildProfileEnvelope(primaryId, b) }

    fun unknownNembBuffer(primaryId: String): ByteArray =
        wrapSingle { b -> buildUnknownEnvelope(primaryId, b) }

    fun collapsedNembBuffer(primaryId: String, reason: String): ByteArray =
        wrapSingle { b -> buildCollapsedEnvelope(primaryId, reason, b) }

    /** Pack the given envelope offsets into a finished NEMB buffer. */
    fun wrapEntries(b: FlatBufferBuilder, envelopeOffsets: IntArray): ByteArray {
        val entries = RefEventEnvelopes.createEntriesVector(b, envelopeOffsets)
        val root = RefEventEnvelopes.createRefEventEnvelopes(b, entries)
        RefEventEnvelopes.finishRefEventEnvelopesBuffer(b, root)
        return b.sizedByteArray()
    }

    private inline fun wrapSingle(buildEnvelope: (FlatBufferBuilder) -> Int): ByteArray {
        val b = FlatBufferBuilder(512)
        return wrapEntries(b, intArrayOf(buildEnvelope(b)))
    }

    // Envelope builders — each returns an offset into the given FlatBufferBuilder.

    fun buildShortNoteEnvelope(
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

    fun buildArticleEnvelope(
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

    fun buildHighlightEnvelope(
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

    fun buildProfileEnvelope(
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

    fun buildUnknownEnvelope(
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

    fun buildCollapsedEnvelope(
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

    /**
     * Build a minimal `ContentTreeWire` (`NFCT`) buffer carrying a single text
     * node so [TypedEmbedSidecarDecoder]'s NFCT sub-buffer path can be tested.
     */
    fun buildTextNfctBuffer(text: String): ByteArray {
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
