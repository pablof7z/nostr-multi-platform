package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.content.ContentTreeWire as FbContentTreeWire
import nmp.content.PlaceholderReason
import nmp.content.RenderMode
import nmp.content.WireNode
import nmp.content.WireNodeKind
import nmp.nip01.NoteRelationCounts as FbNoteRelationCounts
import nmp.nip01.OpFeedSnapshot as FbOpFeedSnapshot
import nmp.nip01.RelationCount as FbRelationCount
import nmp.nip01.RelationCountState
import nmp.nip01.RootCard as FbRootCard
import nmp.nip01.TimelineEventCard as FbTimelineEventCard
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.nmp.android.model.ContentWireNode

@OptIn(ExperimentalUnsignedTypes::class)
class TypedHomeFeedDecoderContractTest {
    @Test
    fun typedDecoderRejectsUnsupportedEnvelopeSchemaVersion() {
        val envelope = TypedProjectionEnvelope(
            key = TypedHomeFeedDecoder.PROJECTION_KEY,
            schemaId = TypedHomeFeedDecoder.SCHEMA_ID,
            schemaVersion = 2u,
            fileIdentifier = TypedHomeFeedDecoder.FILE_IDENTIFIER,
            payload = emptyNofsSnapshot(schemaVersion = 1u),
        )

        assertNull(TypedHomeFeedDecoder.decode(listOf(envelope)))
    }

    @Test
    fun typedDecoderRejectsUnsupportedRootSchemaVersion() {
        assertNull(TypedHomeFeedDecoder.decode(emptyNofsSnapshot(schemaVersion = 2u)))
    }

    @Test
    fun decodeFlatFeedsKeysAuthorAndThreadSidecarsOnly() {
        val authorKey = "nmp.feed.author.${"ab".repeat(32)}"
        val threadKey = "nmp.feed.thread.${"cd".repeat(32)}"
        val nofs = emptyNofsSnapshot(schemaVersion = 1u)
        val envelopes = listOf(
            nofsEnvelope(authorKey, nofs),
            nofsEnvelope(threadKey, nofs),
            // home key is NOT an author/thread prefix → not swept into flatFeeds.
            nofsEnvelope("nmp.feed.home", nofs),
            // wrong schema id under an author prefix → ignored (no entry).
            TypedProjectionEnvelope(
                key = "nmp.feed.author.${"ef".repeat(32)}",
                schemaId = "nmp.nip01.timeline",
                schemaVersion = 1u,
                fileIdentifier = "NFTS",
                payload = nofs,
            ),
            // unrelated typed key → ignored.
            nofsEnvelope("nmp.nip17.dm_inbox", nofs),
        )

        val feeds = TypedHomeFeedDecoder.decodeFlatFeeds(envelopes)

        assertEquals(setOf(authorKey, threadKey), feeds.keys)
    }

    @Test
    fun decodeFlatFeedsSkipsUndecodableSidecar() {
        val authorKey = "nmp.feed.author.${"ab".repeat(32)}"
        val garbled = emptyNofsSnapshot(schemaVersion = 1u).copyOf()
        garbled[4] = 'X'.code.toByte() // clobber the NOFS file identifier
        val feeds = TypedHomeFeedDecoder.decodeFlatFeeds(listOf(nofsEnvelope(authorKey, garbled)))
        assertEquals(emptyMap<String, Any>(), feeds)
    }

    private fun nofsEnvelope(key: String, payload: ByteArray): TypedProjectionEnvelope =
        TypedProjectionEnvelope(
            key = key,
            schemaId = TypedHomeFeedDecoder.SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedHomeFeedDecoder.FILE_IDENTIFIER,
            payload = payload,
        )

    @Test
    fun relationCountsDecodeFromTypedFixture() {
        val snapshot = requireNotNull(
            TypedHomeFeedDecoder.decode(relationCountsFixture()),
        ) { "NOFS relation-count fixture must decode" }

        val card = snapshot.cards.single().card
        val counts = requireNotNull(card.relationCounts) {
            "typed card must carry relation counts"
        }
        assertEquals(2UL, counts.replies.value)
        assertNull(counts.reactions.value)
        assertEquals(3UL, counts.reposts.value)
        assertEquals(1UL, counts.zaps.value)
    }

    @Test
    fun contentTreeVariantDecodePreservesInvoiceImageTitleAndPlaceholderReason() {
        val tree = requireNotNull(
            TypedHomeFeedDecoder.decodeContentTreeBytes(contentTreeVariantFixture()),
        ) { "variant NFCT fixture must decode" }

        assertEquals(listOf(0, 1, 2), tree.roots)
        assertEquals(ContentWireNode.InvoiceNode("Bolt11", "lnbc1demo"), tree.nodes[0])

        val image = tree.nodes[1] as ContentWireNode.ImageNode
        assertEquals("invoice image alt", image.alt)
        assertEquals("Invoice preview", image.title)
        assertEquals("https://example.com/invoice.png", image.src)

        val placeholder = tree.nodes[2] as ContentWireNode.PlaceholderNode
        assertEquals("unresolved_uri", placeholder.reason)
    }

    @Test
    fun genericContentTreePreservesVariantPayloads() {
        val json = org.nmp.android.model.testJson()
        val payload = """
            {
              "nodes": [
                { "kind": "invoice", "invoice": { "Bolt12": "lno1demo" } },
                {
                  "kind": "image",
                  "alt": "image alt",
                  "title": "Image title",
                  "src": "https://example.com/image.png"
                },
                { "kind": "placeholder", "reason": "unresolved_uri" }
              ],
              "roots": [0, 1, 2],
              "mode": "Markdown"
            }
        """.trimIndent()

        val decoded = json.decodeFromString(
            org.nmp.android.model.ContentTreeWire.serializer(),
            payload,
        )

        assertEquals(ContentWireNode.InvoiceNode("Bolt12", "lno1demo"), decoded.nodes[0])
        val image = decoded.nodes[1] as ContentWireNode.ImageNode
        assertEquals("image alt", image.alt)
        assertEquals("Image title", image.title)
        assertEquals("https://example.com/image.png", image.src)
        val placeholder = decoded.nodes[2] as ContentWireNode.PlaceholderNode
        assertEquals("unresolved_uri", placeholder.reason)
    }

    private fun contentTreeVariantFixture(): ByteArray {
        val builder = FlatBufferBuilder(256)
        val invoicePayload = builder.createString("lnbc1demo")
        val imageSrc = builder.createString("https://example.com/invoice.png")
        val imageAlt = builder.createString("invoice image alt")
        val imageTitle = builder.createString("Invoice preview")

        val invoice = WireNode.createWireNode(
            builder,
            WireNodeKind.Invoice,
            0,
            0,
            0,
            0,
            0,
            0,
            0u,
            0,
            0,
            0u,
            0,
            -1L,
            0,
            0,
            0,
            0,
            PlaceholderReason.DepthLimit,
            0u,
            invoicePayload,
        )
        val image = WireNode.createWireNode(
            builder,
            WireNodeKind.Image,
            0,
            imageSrc,
            0,
            0,
            0,
            0,
            0u,
            0,
            0,
            0u,
            0,
            -1L,
            0,
            0,
            imageAlt,
            imageTitle,
            PlaceholderReason.DepthLimit,
            0u,
            0,
        )
        val placeholder = WireNode.createWireNode(
            builder,
            WireNodeKind.Placeholder,
            0,
            0,
            0,
            0,
            0,
            0,
            0u,
            0,
            0,
            0u,
            0,
            -1L,
            0,
            0,
            0,
            0,
            PlaceholderReason.UnresolvedUri,
            0u,
            0,
        )
        val nodes = FbContentTreeWire.createNodesVector(builder, intArrayOf(invoice, image, placeholder))
        val roots = FbContentTreeWire.createRootsVector(builder, uintArrayOf(0u, 1u, 2u))
        val tree = FbContentTreeWire.createContentTreeWire(builder, nodes, roots, RenderMode.Markdown)
        FbContentTreeWire.finishContentTreeWireBuffer(builder, tree)
        return builder.sizedByteArray()
    }

    private fun emptyNofsSnapshot(schemaVersion: UInt): ByteArray {
        val builder = FlatBufferBuilder(128)
        val cards = FbOpFeedSnapshot.createCardsVector(builder, intArrayOf())
        val snapshot = FbOpFeedSnapshot.createOpFeedSnapshot(
            builder,
            schemaVersion,
            cards,
            0,
            false,
            false,
        )
        FbOpFeedSnapshot.finishOpFeedSnapshotBuffer(builder, snapshot)
        return builder.sizedByteArray()
    }

    private fun relationCountsFixture(): ByteArray {
        val builder = FlatBufferBuilder(512)
        val id = builder.createString(hex32(0x22))
        val author = builder.createString(hex32(0x23))
        val content = builder.createString("relation counts")
        val preview = builder.createString("relation counts")

        val replies = FbRelationCount.createRelationCount(builder, RelationCountState.Known, 2UL, 0)
        val reactions = FbRelationCount.createRelationCount(builder, RelationCountState.Loading, 0UL, 0)
        val reposts = FbRelationCount.createRelationCount(builder, RelationCountState.Known, 3UL, 0)
        val zaps = FbRelationCount.createRelationCount(builder, RelationCountState.Known, 1UL, 0)
        val relationCounts = FbNoteRelationCounts.createNoteRelationCounts(
            builder,
            replies,
            reactions,
            reposts,
            zaps,
            0,
        )
        val card = FbTimelineEventCard.createTimelineEventCard(
            builder,
            id,
            author,
            0,
            1u,
            1_700_000_000UL,
            content,
            0,
            0,
            relationCounts,
            false,
            0,
            false,
            0,
            preview,
            0,
            0,
        )
        val attribution = FbRootCard.createAttributionVector(builder, intArrayOf())
        val root = FbRootCard.createRootCard(builder, card, attribution)
        val cards = FbOpFeedSnapshot.createCardsVector(builder, intArrayOf(root))
        val snapshot = FbOpFeedSnapshot.createOpFeedSnapshot(builder, 1u, cards, 0, false, false)
        FbOpFeedSnapshot.finishOpFeedSnapshotBuffer(builder, snapshot)
        return builder.sizedByteArray()
    }

    private fun hex32(byte: Int): String = "%02x".format(byte and 0xff).repeat(32)
}
