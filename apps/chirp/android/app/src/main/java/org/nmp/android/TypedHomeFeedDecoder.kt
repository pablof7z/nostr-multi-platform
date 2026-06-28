package org.nmp.android

import android.util.Log
import nmp.content.ContentTreeWire as FbContentTreeWire
import nmp.content.WireNodeKind
import nmp.content.WireNostrUriKind
import nmp.content.PlaceholderReason as FbPlaceholderReason
import nmp.feed.FeedWindow
import nmp.nip01.OpFeedSnapshot
import nmp.nip01.ReplyAttribution
import nmp.nip01.RelationCountState
import nmp.nip01.RepostAttribution
import nmp.nip01.RootCard
import nmp.nip01.TimelineEventCard
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.ChirpReplyAttribution
import org.nmp.android.model.ChirpRepostAttribution
import org.nmp.android.model.ChirpRootCard
import org.nmp.android.model.ContentTreeWire
import org.nmp.android.model.ContentWireNode
import org.nmp.android.model.NoteRelationCounts
import org.nmp.android.model.RelationCount
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

private val SUPPORTED_SCHEMA_VERSION: UInt = 1u

/**
 * Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers `NOFS` buffer
 * (ADR-0038 Stage T4 / B4 — V-85 complete) into a [ChirpOpFeedSnapshot].
 *
 * ADR-0037 introduced typed FlatBuffers runtime projections. The authorized
 * pilot is `nmp.feed.home`, whose OP-centric view is the nmp-feed
 * `RootFeedSnapshot<TimelineEventCard,
 * Nip10ReplyAttribution>` (`schema_id = "nmp.nip01.opfeed"`, `file_identifier
 * = "NOFS"`). The retired NFTS descriptor (`nmp.nip01.timeline`) is no longer
 * preferred — an `NFTS`-tagged entry is treated as unrecognized and falls
 * through to an empty typed result.
 *
 * Every entry point falls back gracefully — it returns `null` when the
 * projection is absent, carries the wrong schema id, or cannot be verified as
 * a well-formed `NOFS` buffer. Hosts treat `null` as "no typed feed available".
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
     * Dynamic per-view feed key prefixes the producer registers a typed `NOFS`
     * op-feed sidecar under (`nmp.feed.author.<pk>` / `nmp.feed.thread.<id>`) —
     * the SAME shape as `nmp.feed.home`. The producer is
     * `apps/chirp/crates/nmp-app-chirp/src/ffi/interest_feed.rs::register_typed_feed_sidecar`
     * (commit 3dddcd1: "Type transient author/thread interest feeds onto
     * typed_projections sidecar"), which keys each transient feed's NOFS sidecar
     * by the SAME dynamic key the screen reads. `nmp.feed.home` is matched by
     * exact key in [decode]; it is NOT a prefix here, so it never collides.
     *
     * Mirrors iOS `KernelUpdateFrameDecoder.flatFeedKeyPrefixes`.
     */
    private val FLAT_FEED_KEY_PREFIXES = listOf("nmp.feed.author.", "nmp.feed.thread.")

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
        if (projection.schemaVersion != SUPPORTED_SCHEMA_VERSION) return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /**
     * Resolve the per-view author/thread flat feeds from the typed `NOFS`
     * op-feed sidecars ONLY. Each typed
     * envelope whose key carries an author/thread prefix AND whose `schemaId`
     * is the op-feed descriptor is decoded through [decode] (the dynamic feeds
     * are byte-identical in shape to `nmp.feed.home`).
     *
     * Mirrors iOS `KernelUpdateFrameDecoder.overlayTypedFlatFeeds(json:typed:)`
     * after #1062 made the producer emit a typed sidecar for every dynamic feed
     * key, so the typed path is authoritative. Undecodable or non-matching
     * envelopes are skipped (F-05 / ADR-0037 Commitment 4: a malformed sidecar
     * yields no entry; the screen renders its empty-state, never a stale value).
     * Returns an empty map when no author/thread sidecar is present.
     */
    fun decodeFlatFeeds(
        projections: List<TypedProjectionEnvelope>,
    ): Map<String, ChirpOpFeedSnapshot> {
        val result = HashMap<String, ChirpOpFeedSnapshot>()
        for (envelope in projections) {
            if (FLAT_FEED_KEY_PREFIXES.none { envelope.key.startsWith(it) }) continue
            if (envelope.schemaId != SCHEMA_ID) continue
            if (envelope.schemaVersion != SUPPORTED_SCHEMA_VERSION) continue
            if (envelope.payload.isEmpty()) continue
            decode(envelope.payload)?.let { result[envelope.key] = it }
        }
        return result
    }

    /**
     * Decode a raw `NOFS` FlatBuffers buffer into a [ChirpOpFeedSnapshot].
     *
     * Mirrors iOS `TypedHomeFeedDecoder.decode(bytes:)`. Verifies the
     * file_identifier before reading any fields; returns `null` on any parse
     * error so the typed-only host skips that feed projection.
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
            if (snapshot.schemaVersion != SUPPORTED_SCHEMA_VERSION) return null
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
        val relationCounts = card?.relationCounts?.let { makeRelationCounts(it) }
        return ChirpEventCard(
            id = card?.id ?: "",
            authorPubkey = card?.authorPubkey ?: "",
            kind = (card?.kind ?: 0u).toInt(),
            createdAt = (card?.createdAt ?: 0UL).toLong(),
            content = card?.content ?: "",
            contentTree = contentTree,
            relationCounts = relationCounts,
            // ADR-0032: `has_*` companion bool distinguishes "absent (no kind:0
            // yet)" from "present empty string".
            authorDisplayName = if (card?.hasAuthorDisplayName == true) card.authorDisplayName else null,
            authorPictureUrl = if (card?.hasAuthorPictureUrl == true) card.authorPictureUrl else null,
            contentPreview = card?.contentPreview ?: "",
            repostedBy = makeRepostAttribution(card?.repostedBy),
            relayProvenance = buildList {
                val count = card?.relayProvenanceLength ?: 0
                for (i in 0 until count) {
                    card?.relayProvenance(i)?.let { add(it) }
                }
            },
        )
    }

    private fun makeRepostAttribution(entry: RepostAttribution?): ChirpRepostAttribution? {
        if (entry == null) return null
        return ChirpRepostAttribution(
            authorPubkey = entry.authorPubkey ?: "",
            authorDisplayName = if (entry.hasAuthorDisplayName) entry.authorDisplayName else null,
            authorPictureUrl = if (entry.hasAuthorPictureUrl) entry.authorPictureUrl else null,
            noteCreatedAt = entry.noteCreatedAt,
        )
    }

    private fun makeAttribution(entry: ReplyAttribution): ChirpReplyAttribution {
        // ADR-0032 / #1493: flat mirrors removed; read nested authorDisplay.
        val display = entry.authorDisplay
        return ChirpReplyAttribution(
            authorPubkey = entry.authorPubkey ?: "",
            authorDisplayName = if (display?.hasName == true) display.name else null,
            authorPictureUrl = if (display?.hasPictureUrl == true) display.pictureUrl else null,
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
     * Invoice(7) Heading(8) Paragraph(9) BlockQuote(10)
     * CodeBlock(11) List(12) Rule(13) Emphasis(14) Strong(15) InlineCode(16)
     * Link(17) Image(18) SoftBreak(19) HardBreak(20) Placeholder(21).
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

    internal fun decodeContentTreeBytes(bytes: ByteArray): ContentTreeWire? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            decodeContentTree(bb)
        } catch (e: Exception) {
            Log.e(TAG, "NFCT byte decode error: ${e.message}")
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
            WireNodeKind.Invoice -> ContentWireNode.InvoiceNode(
                invoiceKind = invoiceKindString(node.invoiceKind),
                payload = node.invoicePayload.orEmpty(),
            )
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
                title = node.imgTitle,
                // `url` field in the schema encodes `src` for Image nodes
                // (see encode_node in typed_fb.rs: `args.url = src`).
                src = node.url,
            )
            WireNodeKind.SoftBreak -> ContentWireNode.SoftBreakNode
            WireNodeKind.HardBreak -> ContentWireNode.HardBreakNode
            WireNodeKind.Placeholder -> ContentWireNode.PlaceholderNode(placeholderReasonString(node.placeholderReason))
            else -> ContentWireNode.PlaceholderNode() // forward-compat: unknown kind
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

    private fun invoiceKindString(v: UByte): String = when (v) {
        0u.toUByte() -> "Bolt11"
        1u.toUByte() -> "Bolt12"
        2u.toUByte() -> "Cashu"
        else -> "Bolt11"
    }

    private fun placeholderReasonString(v: UByte): String = when (v) {
        FbPlaceholderReason.DepthLimit -> "depth_limit"
        FbPlaceholderReason.UnresolvedUri -> "unresolved_uri"
        else -> "depth_limit"
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

    private fun makeRelationCounts(fb: nmp.nip01.NoteRelationCounts): NoteRelationCounts? {
        val replies = fb.replies ?: return null
        val reactions = fb.reactions ?: return null
        val reposts = fb.reposts ?: return null
        val zaps = fb.zaps ?: return null
        return NoteRelationCounts(
            replies = makeRelationCount(replies),
            reactions = makeRelationCount(reactions),
            reposts = makeRelationCount(reposts),
            zaps = makeRelationCount(zaps),
        )
    }

    private fun makeRelationCount(fb: nmp.nip01.RelationCount): RelationCount {
        return when (fb.state) {
            RelationCountState.Known -> RelationCount.known(fb.count)
            RelationCountState.Loading -> RelationCount.loading()
            else -> RelationCount.loading()
        }
    }
}
