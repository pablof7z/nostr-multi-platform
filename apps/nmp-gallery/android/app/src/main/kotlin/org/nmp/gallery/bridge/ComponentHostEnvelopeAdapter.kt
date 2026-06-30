package org.nmp.gallery.bridge

import org.nmp.gallery.registry.ArticleProjection
import org.nmp.gallery.registry.EmbedKindProjection
import org.nmp.gallery.registry.EmbeddedEventEnvelope
import org.nmp.gallery.registry.HighlightProjection
import org.nmp.gallery.registry.ProfileProjection
import org.nmp.gallery.registry.ShortNoteProjection
import org.nmp.gallery.registry.UnknownProjection

/**
 * App-owned adapter from the Rust-derived `refs.event.envelopes` JSON sidecar
 * to the installed Compose registry value mirror.
 */
fun ResolvedEventEnvelopeWire.toComponentHostEnvelope(): EmbeddedEventEnvelope? {
    val typedProjection = componentProjection()
    if (typedProjection == null && !collapsed) return null
    return EmbeddedEventEnvelope(
        uri = uri,
        primaryId = primaryId,
        depth = depth,
        maxDepth = maxDepth,
        projection = typedProjection,
        collapsed = collapsed,
        collapseReason = collapseReason,
    )
}

private fun ResolvedEventEnvelopeWire.componentProjection(): EmbedKindProjection? =
    when (projectionVariant) {
        "shortNote" -> EmbedKindProjection(
            shortNote = ShortNoteProjection(
                id = projectionString("id") ?: primaryId,
                authorPubkey = projectionString("authorPubkey").orEmpty(),
                createdAt = projectionLong("createdAt") ?: 0,
                content = (projectionString("content") ?: projectionContentText()).orEmpty(),
                mediaUrls = projectionStrings("mediaUrls"),
            ),
        )
        "article" -> EmbedKindProjection(
            article = ArticleProjection(
                id = projectionString("id") ?: primaryId,
                authorPubkey = projectionString("authorPubkey").orEmpty(),
                createdAt = projectionLong("createdAt") ?: 0,
                title = projectionString("title"),
                summary = projectionString("summary"),
                heroImageUrl = projectionString("heroImageUrl"),
                dTag = projectionString("dTag").orEmpty(),
                content = (projectionString("content") ?: projectionContentText()).orEmpty(),
            ),
        )
        "highlight" -> EmbedKindProjection(
            highlight = HighlightProjection(
                id = projectionString("id") ?: primaryId,
                authorPubkey = projectionString("authorPubkey").orEmpty(),
                createdAt = projectionLong("createdAt") ?: 0,
                highlightedText = projectionString("highlightedText").orEmpty(),
                sourceEventId = projectionString("sourceEventId"),
                sourceEventAddr = projectionString("sourceEventAddr"),
                sourceUrl = projectionString("sourceUrl"),
                context = projectionString("context"),
            ),
        )
        "profile" -> EmbedKindProjection(
            profile = ProfileProjection(
                pubkey = projectionString("pubkey") ?: primaryId,
                displayName = projectionString("displayName"),
                pictureUrl = projectionString("pictureUrl"),
                about = projectionString("about"),
                nip05 = projectionString("nip05"),
                lud16 = projectionString("lud16"),
                bannerUrl = projectionString("bannerUrl"),
            ),
        )
        "unknown" -> EmbedKindProjection(
            unknown = UnknownProjection(
                kind = projectionLong("kind")?.toInt() ?: 0,
                authorPubkey = projectionString("authorPubkey").orEmpty(),
                createdAt = projectionLong("createdAt") ?: 0,
                content = (projectionString("content") ?: projectionContentText()).orEmpty(),
                tags = emptyList(),
                altText = projectionString("altText"),
            ),
        )
        else -> null
    }
