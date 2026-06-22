package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.ArticleProjection
import org.nmp.gallery.model.GalleryEmbedEnvelope
import org.nmp.gallery.model.GalleryEmbedKindProjection
import org.nmp.gallery.model.HighlightProjection
import org.nmp.gallery.model.ProfileProjection
import org.nmp.gallery.model.ShortNoteProjection
import org.nmp.gallery.model.UnknownProjection

// ─────────────────────────────────────────────────────────────────────────────
// Gallery kind registry — mirrors the production `NostrKindRegistry` in the
// Android app's `:app` module (org.nmp.android.ui.embed.NostrKindRegistry).
//
// THIN-SHELL (F-CR-04): NO protocol logic, NO kind classification, NO integer
// `when (kind)` dispatch. The Rust `resolve_embed_projection` in the bundle
// builder already selected the correct variant; this object only routes the
// already-typed variant to its Composable.
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Gallery-local embed dispatcher. Consumes a [GalleryEmbedEnvelope] from the
 * bundle (version 3) and renders the correct per-kind composable, matching the
 * production [org.nmp.android.ui.embed.NostrKindRegistry] contract.
 *
 * Called from [WireNodeView] via [GalleryEmbeddedEvent]; the caller owns the
 * outer [Surface] chrome so this composable focuses solely on content dispatch.
 */
@Composable
fun GalleryKindRegistry(projection: GalleryEmbedKindProjection) {
    when (projection) {
        is GalleryEmbedKindProjection.ShortNote -> GalleryShortNoteView(projection.data)
        is GalleryEmbedKindProjection.Article -> GalleryArticleView(projection.data)
        is GalleryEmbedKindProjection.Highlight -> GalleryHighlightView(projection.data)
        is GalleryEmbedKindProjection.Profile -> GalleryProfileView(projection.data)
        is GalleryEmbedKindProjection.Unknown -> GalleryUnknownView(projection.data)
    }
}

/**
 * Full embed card for one [GalleryEmbedEnvelope]. Applies collapsed / loading
 * states and dispatches resolved projections through [GalleryKindRegistry].
 * This is the WireNodeView call-site replacement for the legacy `EmbeddedEvent`
 * (from the deleted `EmbedCard.kt`).
 */
@Composable
fun GalleryEmbeddedEvent(envelope: GalleryEmbedEnvelope?) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 1.dp,
    ) {
        when {
            envelope == null -> GalleryEmbedMissing()
            envelope.collapsed -> GalleryEmbedCollapsed(envelope.collapseReason)
            envelope.projection != null -> Column(Modifier.padding(10.dp)) {
                GalleryKindRegistry(envelope.projection)
            }
            else -> GalleryEmbedMissing()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stub states
// ─────────────────────────────────────────────────────────────────────────────

@Composable
private fun GalleryEmbedCollapsed(reason: String?) {
    Text(
        "Embed ${reason ?: "collapsed"}",
        modifier = Modifier
            .fillMaxWidth()
            .padding(10.dp),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun GalleryEmbedMissing() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(
                1.dp,
                MaterialTheme.colorScheme.outline.copy(alpha = 0.4f),
                RoundedCornerShape(8.dp),
            )
            .background(Color.Gray.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp),
    ) {
        Text(
            "Embed unavailable",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-kind gallery embed views — gallery peers of the production `:app`
// `ShortNoteEmbedView`, `ArticleEmbedView`, `HighlightEmbedView`,
// `ProfileEmbedView`, `UnknownEmbedView`.
// ─────────────────────────────────────────────────────────────────────────────

/** kind:1 short text note embed. */
@Composable
fun GalleryShortNoteView(note: ShortNoteProjection) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        GalleryEmbedByline(
            pubkeyOrUri = note.authorPubkey,
            displayName = note.authorDisplayName,
            caption = "note",
        )
        WireNodeView(tree = note.contentTree, embeds = emptyMap())
        note.mediaUrls.forEach { url ->
            Text(
                url,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
                maxLines = 1,
            )
        }
    }
}

/** kind:30023 long-form article embed. */
@Composable
fun GalleryArticleView(article: ArticleProjection) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        GalleryEmbedByline(
            pubkeyOrUri = article.authorPubkey,
            displayName = article.authorDisplayName,
            caption = "article",
        )
        article.title?.takeIf { it.isNotEmpty() }?.let { title ->
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
        }
        val body = article.summary?.takeIf { it.isNotEmpty() }
        body?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** kind:9802 (NIP-84) highlight embed. */
@Composable
fun GalleryHighlightView(highlight: HighlightProjection) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        GalleryEmbedByline(
            pubkeyOrUri = highlight.authorPubkey,
            displayName = highlight.authorDisplayName,
            caption = "highlight",
        )
        Text(
            "“${highlight.highlightedText}”",
            style = MaterialTheme.typography.bodyLarge.copy(fontStyle = FontStyle.Italic),
            modifier = Modifier.padding(start = 6.dp),
        )
        highlight.sourceUrl?.takeIf { it.isNotEmpty() }?.let { url ->
            Text(
                url,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

/** kind:0 profile metadata embed. */
@Composable
fun GalleryProfileView(profile: ProfileProjection) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        GalleryEmbedByline(
            pubkeyOrUri = profile.pubkey,
            displayName = profile.displayName,
            caption = profile.nip05?.takeIf { it.isNotEmpty() } ?: "profile",
        )
        profile.about?.takeIf { it.isNotEmpty() }?.let { about ->
            Text(
                about,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Fallback embed renderer for unknown kinds. */
@Composable
fun GalleryUnknownView(unknown: UnknownProjection) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        GalleryEmbedByline(
            pubkeyOrUri = unknown.authorPubkey,
            displayName = unknown.authorDisplayName,
            caption = "kind ${unknown.kind}",
        )
        val body = unknown.altText?.takeIf { it.isNotEmpty() }
            ?: unknown.content.takeIf { it.isNotEmpty() }
        body?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared byline component — gallery-local peer of the production EmbedByline
// (org.nmp.android.ui.embed.EmbedChrome). Uses Identicon since the gallery
// has no live profile resolution.
// ─────────────────────────────────────────────────────────────────────────────

@Composable
private fun GalleryEmbedByline(
    pubkeyOrUri: String,
    displayName: String?,
    caption: String,
) {
    Row(
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Identicon(seed = pubkeyOrUri, modifier = Modifier.size(24.dp))
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(
                displayName?.takeIf { it.isNotEmpty() } ?: shortHex(pubkeyOrUri),
                style = MaterialTheme.typography.labelLarge,
                fontWeight = FontWeight.Bold,
            )
            Text(
                caption,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Truncate a hex pubkey / nostr URI to a short display form. */
private fun shortHex(value: String): String {
    val stripped = value.removePrefix("nostr:")
        .removePrefix("npub1")
        .removePrefix("nprofile1")
    if (stripped.length <= 16) return stripped.ifEmpty { "unknown" }
    return "${stripped.take(8)}…${stripped.takeLast(8)}"
}
