package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.registry.ContentTreeWire
import org.nmp.gallery.registry.LocalNostrContentRenderer
import org.nmp.gallery.registry.MediaKind
import org.nmp.gallery.registry.NostrContentRenderer
import org.nmp.gallery.registry.NostrContentView
import org.nmp.gallery.registry.NostrMediaGrid
import org.nmp.gallery.registry.NostrMentionChip
import org.nmp.gallery.registry.NostrQuoteCard
import org.nmp.gallery.registry.NostrQuoteCardModel
import org.nmp.gallery.registry.NostrQuoteCardVariant
import org.nmp.gallery.registry.WireNode
import org.nmp.gallery.registry.WireNostrUri
import org.nmp.gallery.registry.WireNostrUriKind

/**
 * Content-component demos. These do not require relay data — the demo
 * payloads are constructed in-process so the gallery can showcase
 * `ContentTreeWire` / `NostrQuoteCardModel` shapes deterministically.
 *
 * The [LocalNostrContentRenderer] is themed against Material so the
 * component output matches the surrounding chrome.
 */
@Composable
fun ContentComponentPage(componentId: String) {
    val renderer = NostrContentRenderer(
        textColor = MaterialTheme.colorScheme.onSurface,
        secondaryTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
        mentionColor = MaterialTheme.colorScheme.primary,
        hashtagColor = MaterialTheme.colorScheme.tertiary,
        linkColor = MaterialTheme.colorScheme.primary,
    )
    CompositionLocalProvider(LocalNostrContentRenderer provides renderer) {
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
            ContentComponentBody(componentId = componentId)
        }
    }
}

@Composable
private fun ContentComponentBody(componentId: String) {
    when (componentId) {
        "content-core" -> ContentCoreDemo()
        "content-view" -> ContentViewDemo()
        "content-mention-chip" -> MentionChipDemo()
        "content-minimal" -> MinimalContentDemo()
        "content-media-grid" -> MediaGridDemo()
        "content-quote-card" -> QuoteCardDemo()
        else -> Text("Unknown content component: $componentId")
    }
}

@Composable
private fun ContentCoreDemo() {
    NostrContentView(tree = demoTextTree())
}

@Composable
private fun ContentViewDemo() {
    NostrContentView(tree = demoRichTree())
}

@Composable
private fun MentionChipDemo() {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        NostrMentionChip(
            pubkey = DEMO_JACK,
            displayName = "jack",
            avatarUrl = null,
        )
        NostrMentionChip(
            pubkey = DEMO_OTHER,
            displayName = "satoshi",
            avatarUrl = null,
            showsAvatar = false,
        )
    }
}

@Composable
private fun MinimalContentDemo() {
    // No standalone minimal renderer in the current registry — fall back to
    // the standard content view with a short, single-paragraph tree.
    NostrContentView(tree = demoShortTree())
}

@Composable
private fun MediaGridDemo() {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("1 image", style = MaterialTheme.typography.bodySmall)
        NostrMediaGrid(imageUrls = listOf(SAMPLE_IMAGE_1))
        Text("3 images", style = MaterialTheme.typography.bodySmall)
        NostrMediaGrid(
            imageUrls = listOf(SAMPLE_IMAGE_1, SAMPLE_IMAGE_2, SAMPLE_IMAGE_3),
        )
    }
}

@Composable
private fun QuoteCardDemo() {
    val model = NostrQuoteCardModel(
        id = "demo-event-1",
        authorPubkey = DEMO_JACK,
        authorDisplayName = "jack",
        content = "Bitcoin solves this. We're early.",
        createdAtDisplay = "2026-05-25",
    )
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Rich", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = model, variant = NostrQuoteCardVariant.Rich)
        Text("Compact", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = model, variant = NostrQuoteCardVariant.Compact)
        Text("Collapsed", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = model, variant = NostrQuoteCardVariant.Collapsed)
        Text("Missing", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(
            model = NostrQuoteCardModel.Missing.copy(unresolvedUri = "nostr:nevent1…"),
            variant = NostrQuoteCardVariant.Missing,
        )
    }
}

// ── Synthetic content trees ──────────────────────────────────────────────────

private fun demoTextTree(): ContentTreeWire {
    val nodes = listOf<WireNode>(
        WireNode.Text("ContentTreeWire is a flat arena of inline + block nodes."),
        WireNode.Paragraph(children = listOf(0u)),
    )
    return ContentTreeWire(nodes = nodes, roots = listOf(1u))
}

private fun demoShortTree(): ContentTreeWire {
    val nodes = listOf<WireNode>(
        WireNode.Text("Minimal flow renderer demo — single paragraph."),
        WireNode.Paragraph(children = listOf(0u)),
    )
    return ContentTreeWire(nodes = nodes, roots = listOf(1u))
}

private fun demoRichTree(): ContentTreeWire {
    val nodes = listOf<WireNode>(
        // 0
        WireNode.Text("Hello "),
        // 1
        WireNode.Mention(
            uri = WireNostrUri(
                uri = "nostr:npub1demo",
                kind = WireNostrUriKind.Profile,
                primaryId = DEMO_JACK,
            ),
        ),
        // 2
        WireNode.Text(", "),
        // 3
        WireNode.Hashtag(tag = "nostr"),
        // 4
        WireNode.Text(" lives in a flat arena."),
        // 5
        WireNode.Paragraph(children = listOf(0u, 1u, 2u, 3u, 4u)),
        // 6
        WireNode.CodeBlock(info = "kotlin", body = "val world = \"hello\""),
        // 7
        WireNode.Media(urls = listOf(SAMPLE_IMAGE_1), mediaKind = MediaKind.Image),
    )
    return ContentTreeWire(
        nodes = nodes,
        roots = listOf(5u, 6u, 7u),
    )
}

// ── Constants ────────────────────────────────────────────────────────────────

private const val DEMO_JACK =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"

private const val DEMO_OTHER =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245"

private const val SAMPLE_IMAGE_1 = "https://picsum.photos/seed/nmp1/640/360"
private const val SAMPLE_IMAGE_2 = "https://picsum.photos/seed/nmp2/640/360"
private const val SAMPLE_IMAGE_3 = "https://picsum.photos/seed/nmp3/640/360"

private fun labelFor(componentId: String): String = when (componentId) {
    "content-core" -> "ContentTreeWire (synthetic tree)"
    "content-view" -> "NostrContentView (synthetic rich tree)"
    "content-mention-chip" -> "NostrMentionChip (synthetic)"
    "content-minimal" -> "NostrMinimalContentView (synthetic)"
    "content-media-grid" -> "NostrMediaGrid (synthetic)"
    "content-quote-card" -> "NostrQuoteCard (synthetic)"
    else -> componentId
}

