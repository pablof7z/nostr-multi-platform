package org.nmp.android

import android.util.Log
import nmp.content.ContentTreeWire as FbContentTreeWire
import nmp.content.WireNodeKind
import nmp.content.WireNostrUriKind
import nmp.content.PlaceholderReason as FbPlaceholderReason
import nmp.content.RenderMode as FbRenderMode
import nmp.feed.FeedWindow
import nmp.nip01.OpFeedSnapshot
import nmp.nip01.ReplyAttribution
import nmp.nip01.RootCard
import nmp.nip01.TimelineEventCard
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.ChirpReplyAttribution
import org.nmp.android.model.ChirpRootCard
import org.nmp.android.model.ContentTreeWire
import org.nmp.android.model.ContentWireNode
import org.nmp.android.model.TimelineWindowCursor
import org.nmp.android.model.TimelineWindowPage
import org.nmp.android.model.WireNostrUri
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedHomeFeedDecoder"

/** Sentinel: `event_kind = u32::MAX` means `None` (mirrors `EVENT_KIND_NONE` in typed_fb.rs). */
private const val EVENT_KIND_NONE: UInt = UInt.MAX_VALUE

/** Sentinel: `ordered_start = -1` means unordered list (mirrors `ORDERED_START_NONE`). */
private const val ORDERED_START_NONE: Long = -1L

/**
 * Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers `NOFS` buffer
 * (ADR-0038 Stage T4 / B4 — V-85 complete) into a [ChirpOpFeedSnapshot].
 *
 * ADR-0037 introduced typed FlatBuffers runtime projections carried alongside
 * the generic snapshot `payload`. The authorized pilot is `nmp.feed.home`,
 * whose OP-centric view is the nmp-feed `RootFeedSnapshot<TimelineEventCard,
 * Nip10ReplyAttribution>` (`schema_id = "nmp.nip01.opfeed"`, `file_identifier
 * = "NOFS"`). The retired NFTS descriptor (`nmp.nip01.timeline`) is no longer
 * preferred — an `NFTS`-tagged entry is treated as unrecognized and falls
 * through to the generic projection (ADR-0037 Commitment 4).
 *
 * Every entry point falls back gracefully — it returns `null` when the
 * projection is absent, carries the wrong schema id, or cannot be verified as
 * a well-formed `NOFS` buffer. Hosts treat `null` as "no typed feed available"
 * and keep rendering the generic snapshot (ADR-0037 Commitment 4 permanent
 * fallback — the generic `Value` path is never removed).
 *
 * V-85 adds the native Kotlin NFCT decoder (`decodeContentTree`) so
 * [ChirpEventCard.contentTree] is now populated from the embedded
 * `content_tree_bytes` sub-buffer inside each [TimelineEventCard]. The typed
 * path is now the live preferred path; `KernelModel.decodeUpdate` wires it.
 */
object TypedHomeFeedDecoder {

    /** Projection key published by the kernel (`TypedProjection.key`). */
    const val PROJECTION_KEY = "nmp.feed.home"

    /** Schema id carried in `TypedPayload.schema_id` for the NOFS wire. */
    const val SCHEMA_ID = "nmp.nip01.opfeed"

    /** FlatBuffers `file_identifier` for `OpFeedSnapshot`. */
    const val FILE_IDENTIFIER = "NOFS"

    /**
     * Extract and decode the `nmp.feed.home` typed payload from a list of
     * [TypedProjectionEnvelope]s lifted off a snapshot frame.
     *
     * Mirrors iOS `TypedHomeFeedDecoder.decode(from:)`. Returns `null` (no
     * typed feed) when the matching NOFS entry is absent or empty.
     */
    fun decode(projections: List<TypedProjectionEnvelope>): ChirpOpFeedSnapshot? {
        val projection = projections.firstOrNull {
            it.key == PROJECTION_KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /**
     * Decode a raw `NOFS` FlatBuffers buffer into a [ChirpOpFeedSnapshot].
     *
     * Mirrors iOS `TypedHomeFeedDecoder.decode(bytes:)`. Verifies the
     * file_identifier before reading any fields; returns `null` on any parse
     * error so the host falls back to the generic projection.
     */
    fun decode(bytes: ByteArray): ChirpOpFeedSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!OpFeedSnapshot.OpFeedSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "NOFS file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = OpFeedSnapshot.getRootAsOpFeedSnapshot(bb)
            val cards = buildList {
                for (i in 0 until snapshot.cardsLength) {
                    val root = snapshot.cards(i) ?: continue
                    add(makeRootCard(root))
                }
            }
            val page = if (snapshot.hasPage) decodePage(snapshot) else null
            ChirpOpFeedSnapshot(cards = cards, page = page)
        } catch (e: Exception) {
            Log.e(TAG, "NOFS decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    // ── Card mapping ─────────────────────────────────────────────────────────

    private fun makeRootCard(root: RootCard): ChirpRootCard {
        val attribution = buildList {
            for (i in 0 until root.attributionLength) {
                val entry = root.attribution(i) ?: continue
                add(makeAttribution(entry))
            }
        }
        return ChirpRootCard(card = makeCard(root.card), attribution = attribution)
    }

    private fun makeCard(card: TimelineEventCard?): ChirpEventCard {
        // Decode the embedded NFCT content-tree sub-buffer when present.
        // `contentTreeBytesAsByteBuffer` returns null when the vector field
        // is absent (length == 0); guard matches the NFWM pattern in decodePage.
        val contentTree: ContentTreeWire? = if ((card?.contentTreeBytesLength ?: 0) > 0) {
            card?.contentTreeBytesAsByteBuffer?.let { buf ->
                buf.order(ByteOrder.LITTLE_ENDIAN)
                decodeContentTree(buf)
            }
        } else null
        return ChirpEventCard(
            id = card?.id ?: "",
            authorPubkey = card?.authorPubkey ?: "",
            kind = (card?.kind ?: 0u).toInt(),
            createdAt = (card?.createdAt ?: 0UL).toLong(),
            content = card?.content ?: "",
            contentTree = contentTree,
            // ADR-0032: `has_*` companion bool distinguishes "absent (no kind:0
            // yet)" from "present empty string".
            authorDisplayName = if (card?.hasAuthorDisplayName == true) card.authorDisplayName else null,
            authorPictureUrl = if (card?.hasAuthorPictureUrl == true) card.authorPictureUrl else null,
            contentPreview = card?.contentPreview ?: "",
        )
    }

    private fun makeAttribution(entry: ReplyAttribution): ChirpReplyAttribution {
        return ChirpReplyAttribution(
            authorPubkey = entry.authorPubkey ?: "",
            authorDisplayName = if (entry.hasAuthorDisplayName) entry.authorDisplayName else null,
            authorPictureUrl = if (entry.hasAuthorPictureUrl) entry.authorPictureUrl else null,
            replyEventId = entry.replyEventId ?: "",
            replyCreatedAt = entry.replyCreatedAt,
        )
    }

    // ── NFCT content-tree sub-buffer decoder ──────────────────────────────────

    /**
     * Decode an embedded `NFCT` FlatBuffers sub-buffer into a [ContentTreeWire].
     *
     * Verifies the `"NFCT"` file identifier before reading any fields — the
     * same guard used for the `"NFWM"` feed-window buffer in [decodePage].
     * Returns `null` on absent identifier, empty buffer, or any parse error
     * (D1: fail closed, never crash). Raw values only — no display helpers,
     * no relative time, no short-hex formatting (D8).
     *
     * NFCT is generated by `nmp-content::wire::typed_fb::encode_content_tree`
     * (schema: `crates/nmp-content/schema/content_tree.fbs`).
     * All 22 [WireNodeKind] variants are handled:
     * Text(0) Mention(1) EventRef(2) Hashtag(3) Url(4) Media(5) Emoji(6)
     * Invoice(7→Placeholder) Heading(8) Paragraph(9) BlockQuote(10)
     * CodeBlock(11) List(12) Rule(13) Emphasis(14) Strong(15) InlineCode(16)
     * Link(17) Image(18) SoftBreak(19) HardBreak(20) Placeholder(21).
     * Invoice is mapped to [ContentWireNode.PlaceholderNode] to match the
     * generic JSON fallback path (the Kotlin model has no InvoiceNode).
     */
    private fun decodeContentTree(buf: ByteBuffer): ContentTreeWire? {
        if (!FbContentTreeWire.ContentTreeWireBufferHasIdentifier(buf)) {
            Log.e(TAG, "NFCT file_identifier missing")
            return null
        }
        return try {
            val tree = FbContentTreeWire.getRootAsContentTreeWire(buf)
            val nodes = buildList {
                for (i in 0 until tree.nodesLength) {
                    val node = tree.nodes(i) ?: continue
                    add(decodeWireNode(node))
                }
            }
            val roots = buildList {
                for (i in 0 until tree.rootsLength) {
                    add(tree.roots(i).toInt())
                }
            }
            val mode = renderModeFromFb(tree.mode)
            ContentTreeWire(nodes = nodes, roots = roots, mode = mode)
        } catch (e: Exception) {
            Log.e(TAG, "NFCT decode error: ${e.message}")
            null
        }
    }

    /**
     * Map a single FlatBuffers [nmp.content.WireNode] to [ContentWireNode].
     *
     * Dispatch is on `kind` only — several variants share field names (`text`,
     * `children`); the discriminator is the sole authority on which fields are
     * meaningful, mirroring the Rust decode in `typed_fb.rs::decode_node`.
     */
    private fun decodeWireNode(node: nmp.content.WireNode): ContentWireNode {
        return when (node.kind) {
            WireNodeKind.Text -> ContentWireNode.TextNode(node.text.orEmpty())
            WireNodeKind.Mention -> ContentWireNode.MentionNode(decodeNostrUri(node))
            WireNodeKind.EventRef -> ContentWireNode.EventRefNode(decodeNostrUri(node))
            WireNodeKind.Hashtag -> ContentWireNode.HashtagNode(node.tag.orEmpty())
            WireNodeKind.Url -> ContentWireNode.UrlNode(node.url.orEmpty())
            WireNodeKind.Media -> ContentWireNode.MediaNode(
                urls = buildList { for (i in 0 until node.mediaUrlsLength) add(node.mediaUrls(i).orEmpty()) },
                mediaKind = mediaKindString(node.mediaKind),
            )
            WireNodeKind.Emoji -> ContentWireNode.EmojiNode(
                shortcode = node.shortcode.orEmpty(),
                url = node.emojiUrl,
            )
            // Invoice: no InvoiceNode in the Kotlin model; map to Placeholder to
            // match the generic JSON path (`ContentWireNodeSerializer` else branch).
            WireNodeKind.Invoice -> ContentWireNode.PlaceholderNode
            WireNodeKind.Heading -> ContentWireNode.HeadingNode(
                level = node.level.toInt(),
                children = childrenList(node),
            )
            WireNodeKind.Paragraph -> ContentWireNode.ParagraphNode(childrenList(node))
            WireNodeKind.BlockQuote -> ContentWireNode.BlockQuoteNode(childrenList(node))
            WireNodeKind.CodeBlock -> ContentWireNode.CodeBlockNode(
                info = node.codeInfo,
                body = node.text.orEmpty(),
            )
            WireNodeKind.List -> {
                // ordered_start default in schema is -1 (ORDERED_START_NONE = unordered).
                val orderedStart: Long? = if (node.orderedStart == ORDERED_START_NONE) null else node.orderedStart
                ContentWireNode.ListNode(
                    orderedStart = orderedStart,
                    items = buildList {
                        for (i in 0 until node.listItemsLength) {
                            val item = node.listItems(i) ?: continue
                            add(buildList { for (j in 0 until item.childrenLength) add(item.children(j).toInt()) })
                        }
                    },
                )
            }
            WireNodeKind.Rule -> ContentWireNode.RuleNode
            WireNodeKind.Emphasis -> ContentWireNode.EmphasisNode(childrenList(node))
            WireNodeKind.Strong -> ContentWireNode.StrongNode(childrenList(node))
            WireNodeKind.InlineCode -> ContentWireNode.InlineCodeNode(node.text.orEmpty())
            WireNodeKind.Link -> ContentWireNode.LinkNode(
                children = childrenList(node),
                href = node.href,
            )
            WireNodeKind.Image -> ContentWireNode.ImageNode(
                alt = node.alt.orEmpty(),
                // `url` field in the schema encodes `src` for Image nodes
                // (see encode_node in typed_fb.rs: `args.url = src`).
                // `imgTitle` is dropped — the Kotlin model has no title field.
                src = node.url,
            )
            WireNodeKind.SoftBreak -> ContentWireNode.SoftBreakNode
            WireNodeKind.HardBreak -> ContentWireNode.HardBreakNode
            WireNodeKind.Placeholder -> ContentWireNode.PlaceholderNode
            else -> ContentWireNode.PlaceholderNode // forward-compat: unknown kind
        }
    }

    /** Decode the `nostr_uri` sub-table of a Mention or EventRef node. */
    private fun decodeNostrUri(node: nmp.content.WireNode): WireNostrUri {
        val fb = node.nostrUri ?: return WireNostrUri()
        val kind = when (fb.kind) {
            WireNostrUriKind.Profile -> "profile"
            WireNostrUriKind.Event -> "event"
            WireNostrUriKind.Address -> "address"
            else -> "profile"
        }
        // event_kind uses EVENT_KIND_NONE (u32::MAX) as the None sentinel.
        val eventKind: Int? = if (fb.eventKind == EVENT_KIND_NONE) null else fb.eventKind.toInt()
        return WireNostrUri(
            uri = fb.uri.orEmpty(),
            kind = kind,
            primaryId = fb.primaryId.orEmpty(),
            relays = buildList { for (i in 0 until fb.relaysLength) add(fb.relays(i).orEmpty()) },
            author = fb.author,
            eventKind = eventKind,
        )
    }

    /** Collect the `children [uint32]` vector into an `Int` list. */
    private fun childrenList(node: nmp.content.WireNode): List<Int> =
        buildList { for (i in 0 until node.childrenLength) add(node.children(i).toInt()) }

    /**
     * Map a FlatBuffers `media_kind` uint8 to the string the Kotlin model
     * carries (matches `MediaKind` serde PascalCase — no `rename_all` in Rust).
     * Image=0, Video=1, Audio=2.
     */
    private fun mediaKindString(v: UByte): String = when (v) {
        0u.toUByte() -> "Image"
        1u.toUByte() -> "Video"
        2u.toUByte() -> "Audio"
        else -> "Image" // forward-compat default
    }

    /**
     * Map a FlatBuffers `RenderMode` byte to the string the Kotlin model uses.
     * Schema: Auto=0, Markdown=1, Text=2. Text maps to "Plain" — the Rust wire
     * encoder maps `RenderMode::Plain → fb::RenderMode::Text` (value 2).
     */
    private fun renderModeFromFb(v: UByte): String = when (v) {
        nmp.content.RenderMode.Auto -> "Auto"
        nmp.content.RenderMode.Markdown -> "Markdown"
        nmp.content.RenderMode.Text -> "Plain"
        else -> "Auto"
    }

    // ── Feed-window (NFWM) sub-buffer → page ──────────────────────────────────

    /**
     * Decode the embedded `feed_window_bytes` (`NFWM`) sub-buffer and map its
     * `FeedPage` to the [TimelineWindowPage] the renderer paginates on. Returns
     * `null` when the window is absent, malformed, or carries no page (the
     * generic decoder likewise ignores `metrics`, so this maps page only).
     */
    private fun decodePage(snapshot: OpFeedSnapshot): TimelineWindowPage? {
        if (snapshot.feedWindowBytesLength == 0) return null
        // `feedWindowBytesAsByteBuffer` is non-null for this generated table
        // (the `[ubyte]` accessor); the length guard above rules out an empty
        // window, so the slice is a well-formed embedded NFWM buffer.
        val windowBuffer = snapshot.feedWindowBytesAsByteBuffer
        windowBuffer.order(ByteOrder.LITTLE_ENDIAN)
        if (!FeedWindow.FeedWindowBufferHasIdentifier(windowBuffer)) return null
        val window = FeedWindow.getRootAsFeedWindow(windowBuffer)
        val page = window.page ?: return null
        val cursor = page.nextCursor?.let { raw ->
            val id = raw.id ?: return@let null
            TimelineWindowCursor(createdAt = raw.createdAt, id = id)
        }
        return TimelineWindowPage(
            limit = page.limit,
            nextCursor = cursor,
            hasMore = page.hasMore,
            totalBlocks = page.totalBlocks,
        )
    }
}
