package org.nmp.android

import android.util.Log
import nmp.feed.FeedWindow
import nmp.nip01.OpFeedSnapshot
import nmp.nip01.ReplyAttribution
import nmp.nip01.RootCard
import nmp.nip01.TimelineEventCard
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.ChirpReplyAttribution
import org.nmp.android.model.ChirpRootCard
import org.nmp.android.model.TimelineWindowCursor
import org.nmp.android.model.TimelineWindowPage
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedHomeFeedDecoder"

/**
 * Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers `NOFS` buffer
 * (ADR-0038 Stage T4 / B4) into a [ChirpOpFeedSnapshot] — the Android peer of
 * the iOS `TypedHomeFeedDecoder` (PR #755, commit 27c0a101).
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
 * and keep rendering the generic snapshot.
 *
 * CONSUMER STATUS — this decoder is currently exercised only by
 * `OpFeedDecoderTest` (JVM unit test). It is intentionally NOT wired into the
 * render preference (`KernelModel.decodeUpdate`): the typed [TimelineEventCard]
 * carries its content tree as embedded `NFCT` bytes and its relation counts as
 * a typed sub-table, but Android has no Kotlin `NFCT` decoder — so the mapped
 * [ChirpEventCard.contentTree] stays null here (and Android does not model
 * relation counts at all). Decoding the typed path into the render would show
 * blank content. Flipping the runtime preference is a follow-up that first
 * needs a Kotlin `NFCT` decoder (the same V-84-class follow-up that gates iOS).
 * This matches the iOS T3 decoder-only posture exactly.
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
        return ChirpEventCard(
            id = card?.id ?: "",
            authorPubkey = card?.authorPubkey ?: "",
            kind = (card?.kind ?: 0u).toInt(),
            createdAt = (card?.createdAt ?: 0UL).toLong(),
            content = card?.content ?: "",
            // The typed card carries its content tree as embedded NFCT bytes
            // (`content_tree_bytes`); Android has no Kotlin NFCT decoder, so this
            // stays null here. The generic `Value` path fills it from JSON. See
            // the file header — render-completeness for the typed path is a
            // follow-up. The field is nullable in the model, so null is valid.
            contentTree = null,
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
