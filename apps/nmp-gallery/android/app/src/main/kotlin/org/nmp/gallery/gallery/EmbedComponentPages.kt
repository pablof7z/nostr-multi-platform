package org.nmp.gallery.gallery

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.ClaimedEventWire
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.bridge.GalleryShowcaseReferences
import org.nmp.gallery.registry.NostrProfileName

/**
 * Showcase pages for the kind-dispatch embed renderers (ADR-0034 / M16).
 *
 * Each page claims a real event URI from the showcase references, waits for
 * the kernel to resolve it via relay, and displays the event data from
 * `model.claimedEvents`. Shows kind, author pubkey (truncated), and content
 * preview.
 */

@Composable
fun EmbedComponentPage(
    model: GalleryModel,
    componentId: String,
) {
    val claimedEvents by model.claimedEvents.collectAsState()
    val showcase = model.showcase

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = labelFor(componentId),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        EmbedComponentBody(
            componentId = componentId,
            showcase = showcase,
            claimedEvents = claimedEvents,
            model = model,
        )
    }
}

@Composable
private fun EmbedComponentBody(
    componentId: String,
    showcase: GalleryShowcaseReferences,
    claimedEvents: Map<String, ClaimedEventWire>,
    model: GalleryModel,
) {
    when (componentId) {
        "embed-article" -> ArticleEmbedPage(showcase, claimedEvents, model)
        "embed-profile" -> ProfileEmbedPage(showcase, model)
        "embed-note" -> NoteEmbedPage(showcase, claimedEvents, model)
        "embed-highlight" -> HighlightEmbedPage(showcase, claimedEvents, model)
        else -> Text("Unknown embed component: $componentId")
    }
}

// ── Article — kind:30023 ────────────────────────────────────────────

@Composable
private fun ArticleEmbedPage(
    showcase: GalleryShowcaseReferences,
    claimedEvents: Map<String, ClaimedEventWire>,
    model: GalleryModel,
) {
    val articleUri = showcase.article.uri
    val articlePrimaryId = showcase.article.primaryId

    DisposableEffect(articleUri) {
        model.claimEvent(articleUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(articleUri, GalleryModel.CONSUMER_ID)
        }
    }

    val article = claimedEvents[articlePrimaryId]
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Article embed — kind:30023 via NostrKindRegistry",
            style = MaterialTheme.typography.bodySmall,
        )
        EventDisplayCard(
            event = article,
            kind = showcase.article.kind,
            placeholder = "Fetching article from relay…",
        )
        Text(
            "The renderer fires `claim` on the article naddr; the kernel resolves kind:30023 and the typed ArticleProjection flows through EmbedHost.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── Profile — inline npub mention chip ────────────────────────────────────────────

@Composable
private fun ProfileEmbedPage(
    showcase: GalleryShowcaseReferences,
    model: GalleryModel,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Inline profile mention — kind:0 via mention chip",
            style = MaterialTheme.typography.bodySmall,
        )
        EventDisplayCard(
            event = null,
            kind = 0L,
            placeholder = "Profile: ${showcase.profile.npubShort}",
        )
        Text(
            "Profile mentions resolve via projections.mention_profiles — the same kind:0 path the user-* pages use. No embed claim is required for `npub:` URIs.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── Note — kind:1 short text note via nevent ────────────────────────────────────────────

@Composable
private fun NoteEmbedPage(
    showcase: GalleryShowcaseReferences,
    claimedEvents: Map<String, ClaimedEventWire>,
    model: GalleryModel,
) {
    val noteUri = showcase.note.uri
    val notePrimaryId = showcase.note.primaryId

    DisposableEffect(noteUri) {
        model.claimEvent(noteUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(noteUri, GalleryModel.CONSUMER_ID)
        }
    }

    val note = claimedEvents[notePrimaryId]
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Note embed — kind:1 via NostrKindRegistry",
            style = MaterialTheme.typography.bodySmall,
        )
        EventDisplayCard(
            event = note,
            kind = showcase.note.kind,
            placeholder = "Fetching note from relay…",
        )
        Text(
            "nevent1… URIs resolve via the same `claim_event` path. The default short-note renderer paints author + content.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── Highlight — kind:9802 via nevent ────────────────────────────────────────────

@Composable
private fun HighlightEmbedPage(
    showcase: GalleryShowcaseReferences,
    claimedEvents: Map<String, ClaimedEventWire>,
    model: GalleryModel,
) {
    val highlightUri = showcase.highlight.uri
    val highlightPrimaryId = showcase.highlight.primaryId

    DisposableEffect(highlightUri) {
        model.claimEvent(highlightUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(highlightUri, GalleryModel.CONSUMER_ID)
        }
    }

    val highlight = claimedEvents[highlightPrimaryId]
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Highlight embed — kind:9802 via HighlightEmbed renderer",
            style = MaterialTheme.typography.bodySmall,
        )
        EventDisplayCard(
            event = highlight,
            kind = showcase.highlight.kind,
            placeholder = "Fetching highlight from relay…",
        )
        Text(
            "NIP-84 highlights render as a pull-quote with optional source link. The kernel resolves kind:9802; HighlightEmbed paints the typed projection.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── Shared event display card ────────────────────────────────────────────

@Composable
private fun EventDisplayCard(
    event: ClaimedEventWire?,
    kind: Long,
    placeholder: String,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                color = MaterialTheme.colorScheme.secondaryContainer,
                shape = RoundedCornerShape(12.dp),
            )
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (event != null) {
            Text(
                "kind: $kind",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
            // Self-claiming byline: the name component owns claiming the
            // author's kind:0 — the kernel never fetches it.
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    "author:",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
                NostrProfileName(
                    pubkey = event.authorPubkey,
                    style = LocalTextStyle.current.copy(
                        fontWeight = MaterialTheme.typography.bodySmall.fontWeight,
                    ),
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
            }
            val contentPreview = if (event.content.length > 100) {
                event.content.take(97) + "…"
            } else {
                event.content
            }
            Text(
                contentPreview,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        } else {
            Text(
                placeholder,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
    }
}

private fun labelFor(componentId: String): String = when (componentId) {
    "embed-article" -> "Article embed — kind:30023"
    "embed-profile" -> "Profile embed — kind:0"
    "embed-note" -> "Note embed — kind:1"
    "embed-highlight" -> "Highlight embed — kind:9802"
    else -> componentId
}
