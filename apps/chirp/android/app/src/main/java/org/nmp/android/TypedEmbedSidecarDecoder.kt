package org.nmp.android

import android.util.Log
import nmp.embed.RefEventEnvelopes
import nmp.embed.EmbedProjectionKind
import nmp.embed.EmbeddedEventEnvelope as FbEmbeddedEventEnvelope
import nmp.embed.EmbedKindProjection as FbEmbedKindProjection
import org.nmp.android.model.ArticleProjectionEntry
import org.nmp.android.model.EmbedEnvelopeEntry
import org.nmp.android.model.EmbedKindProjectionEntry
import org.nmp.android.model.HighlightProjectionEntry
import org.nmp.android.model.ProfileProjectionEntry
import org.nmp.android.model.ShortNoteProjectionEntry
import org.nmp.android.model.UnknownProjectionEntry
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedEmbedSidecarDecoder"

/**
 * Typed-first decoder for the kernel-owned `refs.event.envelopes` snapshot
 * projection (`NEMB` / `nmp.embed.RefEventEnvelopes`) — the Android peer of
 * iOS `TypedProjectionGlueEmbed.refEventEnvelopes(_:)`.
 *
 * DECODE-ONLY: the kernel resolves all embed projections on the Rust side
 * (`crates/nmp-content/src/embed_projection/`). Zero kind dispatch, zero
 * tag/JSON parsing, zero protocol logic in Kotlin — D0 thin-shell rule
 * (#1283 / #1335 item 2). The `EmbedProjectionKind` discriminant selects which
 * already-populated payload table to copy; the plain-text `content` for
 * text-body variants is extracted from the NFCT sub-buffer by reusing the
 * existing [TypedHomeFeedDecoder.decodeContentTreeBytes] codec (no duplication).
 *
 * ADR-0037 Commitment 4: returns an empty map when the NEMB sidecar is absent,
 * carries the wrong file identifier, or cannot be verified — never crashes.
 *
 * Schema: `crates/nmp-content/schema/embed_sidecar.fbs`
 * Kotlin bindings: `apps/chirp/android/app/src/main/java/nmp/embed/` (flatc 25.2.10)
 */
object TypedEmbedSidecarDecoder {

    /** Projection key published by the kernel (`TypedProjection.key`). */
    const val PROJECTION_KEY = "refs.event.envelopes"

    /** Schema id carried in `TypedPayload.schema_id`. */
    const val SCHEMA_ID = "refs.event.envelopes"

    /** FlatBuffers `file_identifier` for `RefEventEnvelopes`. */
    const val FILE_IDENTIFIER = "NEMB"

    /**
     * Extract and decode the `refs.event.envelopes` typed payload from a list
     * of [TypedProjectionEnvelope]s lifted off a snapshot frame.
     *
     * Returns an empty map when the matching NEMB entry is absent or undecodable
     * (ADR-0037 Commitment 4 permanent fail-closed fallback).
     */
    fun decode(projections: List<TypedProjectionEnvelope>): Map<String, EmbedEnvelopeEntry> {
        val projection = projections.firstOrNull {
            it.key == PROJECTION_KEY && it.schemaId == SCHEMA_ID
        } ?: return emptyMap()
        if (projection.payload.isEmpty()) return emptyMap()
        return decode(projection.payload)
    }

    /**
     * Decode a raw `NEMB` FlatBuffers buffer into a `[primaryId → EmbedEnvelopeEntry]`
     * map.
     *
     * Mirrors iOS `TypedProjectionGlueEmbed.refEventEnvelopes(_:)`. Verifies
     * the `NEMB` file identifier before reading any fields; returns an empty map
     * on any parse error so the host renders empty-state, never stale values
     * (D1: fail closed, never crash). Raw values only — no display helpers,
     * no relative time (D8).
     */
    fun decode(bytes: ByteArray): Map<String, EmbedEnvelopeEntry> {
        if (bytes.isEmpty()) return emptyMap()
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!RefEventEnvelopes.RefEventEnvelopesBufferHasIdentifier(bb)) {
                Log.e(TAG, "NEMB file_identifier missing (${bytes.size} bytes)")
                return emptyMap()
            }
            val root = RefEventEnvelopes.getRootAsRefEventEnvelopes(bb)
            val result = HashMap<String, EmbedEnvelopeEntry>(root.entriesLength * 2)
            for (i in 0 until root.entriesLength) {
                val entry: FbEmbeddedEventEnvelope = root.entries(i) ?: continue
                val primaryId = try { entry.primaryId } catch (e: AssertionError) {
                    // FlatBuffers marks primaryId as `required`; an absent value
                    // throws AssertionError — skip this entry (D1: fail closed).
                    Log.w(TAG, "NEMB entry missing required primaryId at index $i")
                    continue
                }
                if (primaryId.isEmpty()) continue
                val projection = entry.projection?.let { mapProjection(it) } ?: continue
                result[primaryId] = EmbedEnvelopeEntry(
                    primaryId = primaryId,
                    uri = entry.uri ?: "",
                    depth = entry.depth.toInt(),
                    maxDepth = entry.maxDepth.toInt(),
                    collapsed = entry.collapsed,
                    collapseReason = if (entry.hasCollapseReason) entry.collapseReason else null,
                    projection = projection,
                )
            }
            result
        } catch (e: Exception) {
            Log.e(TAG, "NEMB decode error: ${e.message} bytes=${bytes.size}")
            emptyMap()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Kind dispatch — mirrors iOS `TypedProjectionGlueEmbed.mapProjection(_:)`
    // DECODE-ONLY: field copy + enum re-tagging; no resolution logic (D0).
    // ─────────────────────────────────────────────────────────────────────────

    private fun mapProjection(wire: FbEmbedKindProjection): EmbedKindProjectionEntry? {
        return when (wire.kind) {
            EmbedProjectionKind.ShortNote -> {
                val p = wire.shortNote ?: return null
                EmbedKindProjectionEntry(
                    shortNote = ShortNoteProjectionEntry(
                        id = p.id ?: "",
                        authorPubkey = p.authorPubkey ?: "",
                        authorDisplayName = if (p.hasAuthorDisplayName) p.authorDisplayName else null,
                        authorPictureUrl = if (p.hasAuthorPictureUrl) p.authorPictureUrl else null,
                        createdAt = p.createdAt.toLong(),
                        content = if (p.contentTreeLength > 0)
                            plainTextFromContentTree(p.contentTreeAsByteBuffer, p.contentTreeLength) else "",
                        mediaUrls = buildList {
                            for (i in 0 until p.mediaUrlsLength) add(p.mediaUrls(i) ?: "")
                        },
                    )
                )
            }
            EmbedProjectionKind.Article -> {
                val p = wire.article ?: return null
                EmbedKindProjectionEntry(
                    article = ArticleProjectionEntry(
                        id = p.id ?: "",
                        authorPubkey = p.authorPubkey ?: "",
                        authorDisplayName = if (p.hasAuthorDisplayName) p.authorDisplayName else null,
                        authorPictureUrl = if (p.hasAuthorPictureUrl) p.authorPictureUrl else null,
                        createdAt = p.createdAt.toLong(),
                        title = if (p.hasTitle) p.title else null,
                        summary = if (p.hasSummary) p.summary else null,
                        heroImageUrl = if (p.hasHeroImageUrl) p.heroImageUrl else null,
                        dTag = p.dTag ?: "",
                        content = if (p.contentTreeLength > 0)
                            plainTextFromContentTree(p.contentTreeAsByteBuffer, p.contentTreeLength) else "",
                    )
                )
            }
            EmbedProjectionKind.Highlight -> {
                val p = wire.highlight ?: return null
                EmbedKindProjectionEntry(
                    highlight = HighlightProjectionEntry(
                        id = p.id ?: "",
                        authorPubkey = p.authorPubkey ?: "",
                        authorDisplayName = if (p.hasAuthorDisplayName) p.authorDisplayName else null,
                        createdAt = p.createdAt.toLong(),
                        highlightedText = p.highlightedText ?: "",
                        sourceEventId = if (p.hasSourceEventId) p.sourceEventId else null,
                        sourceEventAddr = if (p.hasSourceEventAddr) p.sourceEventAddr else null,
                        sourceUrl = if (p.hasSourceUrl) p.sourceUrl else null,
                        context = if (p.hasContext) p.context else null,
                    )
                )
            }
            EmbedProjectionKind.Profile -> {
                val p = wire.profile ?: return null
                EmbedKindProjectionEntry(
                    profile = ProfileProjectionEntry(
                        pubkey = p.pubkey ?: "",
                        displayName = if (p.hasDisplayName) p.displayName else null,
                        pictureUrl = if (p.hasPictureUrl) p.pictureUrl else null,
                        about = if (p.hasAbout) p.about else null,
                        nip05 = if (p.hasNip05) p.nip05 else null,
                        lud16 = if (p.hasLud16) p.lud16 else null,
                        bannerUrl = if (p.hasBannerUrl) p.bannerUrl else null,
                    )
                )
            }
            EmbedProjectionKind.Unknown -> {
                val p = wire.unknown ?: return null
                EmbedKindProjectionEntry(
                    unknown = UnknownProjectionEntry(
                        kind = p.kind.toInt(),
                        authorPubkey = p.authorPubkey ?: "",
                        authorDisplayName = if (p.hasAuthorDisplayName) p.authorDisplayName else null,
                        authorPictureUrl = if (p.hasAuthorPictureUrl) p.authorPictureUrl else null,
                        createdAt = p.createdAt.toLong(),
                        content = p.content ?: "",
                        tags = buildList {
                            for (i in 0 until p.tagsLength) {
                                val row = p.tags(i) ?: continue
                                add(buildList {
                                    for (j in 0 until row.valuesLength) add(row.values(j) ?: "")
                                })
                            }
                        },
                        altText = if (p.hasAltText) p.altText else null,
                    )
                )
            }
            else -> {
                // Forward-compat: unknown future kind — skip rather than crash (D1).
                Log.w(TAG, "NEMB unknown EmbedProjectionKind ${wire.kind}")
                null
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // NFCT content-tree plain-text helper
    //
    // Reuses [TypedHomeFeedDecoder.decodeContentTreeBytes] — the SAME NFCT codec
    // that the home-feed decoder already ships. This avoids duplicating the
    // content-tree decode logic (Article IX: integration-first reuse).
    // The ByteBuffer from a `[ubyte]` accessor covers only the vector bytes;
    // we materialise it to a ByteArray and delegate.
    // ─────────────────────────────────────────────────────────────────────────

    private fun plainTextFromContentTree(buf: ByteBuffer, length: Int): String {
        if (length == 0) return ""
        return try {
            buf.order(ByteOrder.LITTLE_ENDIAN)
            val bytes = ByteArray(buf.remaining())
            buf.get(bytes)
            TypedHomeFeedDecoder.decodeContentTreeBytes(bytes)?.plainText() ?: ""
        } catch (e: Exception) {
            Log.w(TAG, "NFCT plain-text extraction failed: ${e.message}")
            ""
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ContentTreeWire plain-text flattener (Android peer of iOS ContentTreeWire
// extension in TypedProjectionGlueEmbed.swift). Kept here alongside its sole
// caller (TypedEmbedSidecarDecoder) rather than adding to TypedHomeFeedDecoder
// which is already at its LOC baseline.
// ─────────────────────────────────────────────────────────────────────────────

private fun org.nmp.android.model.ContentTreeWire.plainText(): String {
    val pieces = mutableListOf<String>()
    for (root in roots) {
        appendTextOfNode(root, pieces)
    }
    return pieces.joinToString("").trim()
}

private fun org.nmp.android.model.ContentTreeWire.appendTextOfNode(
    index: Int,
    pieces: MutableList<String>,
) {
    val node = nodes.getOrNull(index) ?: return
    when (node) {
        is org.nmp.android.model.ContentWireNode.TextNode ->
            pieces.add(node.text)
        is org.nmp.android.model.ContentWireNode.UrlNode ->
            pieces.add(node.url)
        is org.nmp.android.model.ContentWireNode.InlineCodeNode ->
            pieces.add(node.code)
        is org.nmp.android.model.ContentWireNode.HashtagNode ->
            pieces.add("#${node.tag}")
        is org.nmp.android.model.ContentWireNode.MentionNode ->
            pieces.add(node.uri.uri)
        is org.nmp.android.model.ContentWireNode.EventRefNode ->
            pieces.add(node.uri.uri)
        is org.nmp.android.model.ContentWireNode.CodeBlockNode -> {
            pieces.add(node.body)
            pieces.add("\n")
        }
        is org.nmp.android.model.ContentWireNode.SoftBreakNode ->
            pieces.add(" ")
        is org.nmp.android.model.ContentWireNode.HardBreakNode ->
            pieces.add("\n")
        is org.nmp.android.model.ContentWireNode.HeadingNode -> {
            for (child in node.children) appendTextOfNode(child, pieces)
            pieces.add("\n")
        }
        is org.nmp.android.model.ContentWireNode.ParagraphNode -> {
            for (child in node.children) appendTextOfNode(child, pieces)
            pieces.add("\n")
        }
        is org.nmp.android.model.ContentWireNode.BlockQuoteNode ->
            for (child in node.children) appendTextOfNode(child, pieces)
        is org.nmp.android.model.ContentWireNode.EmphasisNode ->
            for (child in node.children) appendTextOfNode(child, pieces)
        is org.nmp.android.model.ContentWireNode.StrongNode ->
            for (child in node.children) appendTextOfNode(child, pieces)
        is org.nmp.android.model.ContentWireNode.LinkNode ->
            for (child in node.children) appendTextOfNode(child, pieces)
        is org.nmp.android.model.ContentWireNode.ListNode ->
            for (item in node.items) {
                for (child in item) appendTextOfNode(child, pieces)
                pieces.add("\n")
            }
        // Media, emoji, invoice, image, rule, placeholder contribute nothing
        // (mirrors iOS appendText(ofNodeAt:into:) cases).
        else -> Unit
    }
}
