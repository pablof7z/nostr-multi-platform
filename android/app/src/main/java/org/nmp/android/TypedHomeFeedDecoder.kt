package org.nmp.android

import android.util.Log
import nmp.nip01.ModularTimelineSnapshot
import nmp.nip01.TimelineBlockEntry
import nmp.nip01.TimelineBlockKind
import nmp.nip01.TimelineEventCard
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ChirpTimelineSnapshot
import org.nmp.android.model.ModuleTimelineBlock
import org.nmp.android.model.StandaloneTimelineBlock
import org.nmp.android.model.TimelineBlock
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedHomeFeedDecoder"

/**
 * Decodes the typed `nmp.feed.home` sidecar from a FlatBuffers NFTS buffer.
 *
 * Direct port of iOS `TypedHomeFeedDecoder`. ADR-0037 introduced typed
 * FlatBuffers runtime projections carried alongside the generic snapshot
 * `payload`. The authorized pilot is `nmp.feed.home`, whose full assembled
 * view is the nmp-nip01 `ModularTimelineSnapshot` (schema_id =
 * "nmp.nip01.timeline", file_identifier = "NFTS").
 *
 * Falls back gracefully — returns an empty [ChirpTimelineSnapshot] when the
 * projection is absent, carries the wrong schema id, or cannot be verified as
 * a well-formed NFTS buffer. Callers treat this as "no typed feed available"
 * and keep rendering whatever they had.
 */
object TypedHomeFeedDecoder {

    /** Projection key published by the kernel (`TypedProjection.key`). */
    const val PROJECTION_KEY = "nmp.feed.home"

    /** Schema id carried in `TypedPayload.schema_id` for the NFTS wire. */
    const val SCHEMA_ID = "nmp.nip01.timeline"

    /** FlatBuffers `file_identifier` for `ModularTimelineSnapshot`. */
    const val FILE_IDENTIFIER = "NFTS"

    /**
     * Extract and decode the `nmp.feed.home` typed payload from a list of
     * [TypedProjectionEnvelope]s lifted off a snapshot frame.
     *
     * Mirrors iOS `TypedHomeFeedDecoder.decode(from:)`.
     */
    fun decode(projections: List<TypedProjectionEnvelope>): ChirpTimelineSnapshot {
        val projection = projections.firstOrNull {
            it.key == PROJECTION_KEY && it.schemaId == SCHEMA_ID
        } ?: return ChirpTimelineSnapshot()
        if (projection.payload.isEmpty()) return ChirpTimelineSnapshot()
        return decode(projection.payload)
    }

    /**
     * Decode a raw NFTS FlatBuffers buffer into a [ChirpTimelineSnapshot].
     *
     * Mirrors iOS `TypedHomeFeedDecoder.decode(bytes:)`. Verifies the
     * file_identifier before reading any fields; returns an empty snapshot on
     * any parse error.
     */
    fun decode(bytes: ByteArray): ChirpTimelineSnapshot {
        if (bytes.isEmpty()) return ChirpTimelineSnapshot()
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!ModularTimelineSnapshot.ModularTimelineSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "NFTS file_identifier missing (${bytes.size} bytes)")
                return ChirpTimelineSnapshot()
            }
            val snapshot = ModularTimelineSnapshot.getRootAsModularTimelineSnapshot(bb)
            val blocks = buildList {
                for (i in 0 until snapshot.blocksLength) {
                    val entry = snapshot.blocks(i) ?: continue
                    add(makeBlock(entry))
                }
            }
            val cards = buildList {
                for (i in 0 until snapshot.cardsLength) {
                    val card = snapshot.cards(i) ?: continue
                    makeCard(card)?.let { add(it) }
                }
            }
            ChirpTimelineSnapshot(blocks = blocks, cards = cards)
        } catch (e: Exception) {
            Log.e(TAG, "NFTS decode error: ${e.message} bytes=${bytes.size}")
            ChirpTimelineSnapshot()
        }
    }

    private fun makeBlock(entry: TimelineBlockEntry): TimelineBlock {
        return when (entry.kind) {
            TimelineBlockKind.Standalone -> {
                val id = entry.standaloneId ?: ""
                StandaloneTimelineBlock(eventId = id)
            }
            TimelineBlockKind.Module -> {
                val eventIds = buildList {
                    for (i in 0 until entry.moduleEventIdsLength) {
                        val blockEventId = entry.moduleEventIds(i) ?: continue
                        val id = blockEventId.id ?: continue
                        add(id)
                    }
                }
                ModuleTimelineBlock(events = eventIds, hasGap = entry.moduleHasGap)
            }
            else -> StandaloneTimelineBlock(eventId = "")
        }
    }

    private fun makeCard(card: TimelineEventCard): ChirpEventCard? {
        // A card without an id is unusable for diffing/rendering — drop it.
        val id = card.id ?: return null
        return ChirpEventCard(
            id = id,
            authorPubkey = card.authorPubkey ?: "",
            kind = card.kind.toInt(),
            createdAt = card.createdAt.toLong(),
            content = card.content ?: "",
            // Honour the has_* companion bools: absent means no kind:0 seen yet.
            authorDisplayName = if (card.hasAuthorDisplayName) card.authorDisplayName else null,
            authorPictureUrl = if (card.hasAuthorPictureUrl) card.authorPictureUrl else null,
            contentPreview = card.contentPreview ?: "",
        )
    }
}
