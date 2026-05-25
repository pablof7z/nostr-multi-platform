package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.GalleryModel
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
import org.nmp.gallery.registry.ProfileWire
import org.nmp.gallery.registry.WireNode
import org.nmp.gallery.registry.WireNostrUri
import org.nmp.gallery.registry.WireNostrUriKind
import org.nmp.gallery.registry.defaultMentionLabel

@Composable
fun ContentComponentPage(model: GalleryModel, componentId: String) {
    val profileMap by model.profileMap.collectAsState()

    LaunchedEffect(Unit) {
        model.claimProfile(DEMO_PUBKEY, GalleryModel.CONSUMER_ID)
        model.claimProfile(DEMO_OTHER_PUBKEY, GalleryModel.CONSUMER_ID)
    }

    val renderer = NostrContentRenderer(
        textColor = MaterialTheme.colorScheme.onSurface,
        secondaryTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
        mentionColor = MaterialTheme.colorScheme.primary,
        hashtagColor = MaterialTheme.colorScheme.tertiary,
        linkColor = MaterialTheme.colorScheme.primary,
    )

    val mentionLabel: (WireNostrUri) -> String = { uri ->
        profileMap[uri.primaryId]?.displayName
            ?: defaultMentionLabel(uri)
    }

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
            ContentComponentBody(
                componentId = componentId,
                profileMap = profileMap,
                mentionLabel = mentionLabel,
            )
        }
    }
}

@Composable
private fun ContentComponentBody(
    componentId: String,
    profileMap: Map<String, ProfileWire>,
    mentionLabel: (WireNostrUri) -> String,
) {
    when (componentId) {
        "content-core" -> ContentCoreDemo(mentionLabel)
        "content-view" -> ContentViewDemo(mentionLabel)
        "content-mention-chip" -> MentionChipDemo(profileMap)
        "content-minimal" -> MinimalContentDemo()
        "content-media-grid" -> MediaGridDemo()
        "content-quote-card" -> QuoteCardDemo(profileMap)
        else -> Text("Unknown content component: $componentId")
    }
}

@Composable
private fun ContentCoreDemo(mentionLabel: (WireNostrUri) -> String) {
    NostrContentView(tree = demoTextTree(), mentionLabel = mentionLabel)
}

@Composable
private fun ContentViewDemo(mentionLabel: (WireNostrUri) -> String) {
    NostrContentView(tree = demoRichTree(), mentionLabel = mentionLabel)
}

@Composable
private fun MentionChipDemo(profileMap: Map<String, ProfileWire>) {
    val primary = profileMap[DEMO_PUBKEY]
    val other = profileMap[DEMO_OTHER_PUBKEY]
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Live kernel-resolved profile", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = DEMO_PUBKEY,
            displayName = primary?.displayName,
            avatarUrl = primary?.pictureUrl,
        )
        Text("Second profile (resolved)", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = DEMO_OTHER_PUBKEY,
            displayName = other?.displayName,
            avatarUrl = other?.pictureUrl,
        )
        Text("Identicon fallback (unknown pubkey)", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = "deadbeefcafebabedeadbeefcafebabe",
            displayName = null,
        )
        Text("No avatar variant", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = DEMO_PUBKEY,
            displayName = primary?.displayName,
            showsAvatar = false,
        )
    }
}

@Composable
private fun MinimalContentDemo() {
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
private fun QuoteCardDemo(profileMap: Map<String, ProfileWire>) {
    val profile = profileMap[DEMO_PUBKEY]
    val quoteModel = NostrQuoteCardModel(
        id = "demo-event-1",
        authorPubkey = DEMO_PUBKEY,
        authorDisplayName = profile?.displayName,
        content = "Bitcoin solves this. We're early.",
        createdAtDisplay = "2026-05-25",
    )
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Rich", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Rich)
        Text("Compact", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Compact)
        Text("Collapsed", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Collapsed)
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
                uri = "nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft",
                kind = WireNostrUriKind.Profile,
                primaryId = DEMO_PUBKEY,
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

private const val DEMO_PUBKEY = GalleryModel.DEMO_PUBKEY

// jb55 (William Casarin)
private const val DEMO_OTHER_PUBKEY =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245"

private const val SAMPLE_IMAGE_1 = "https://picsum.photos/seed/nmp1/640/360"
private const val SAMPLE_IMAGE_2 = "https://picsum.photos/seed/nmp2/640/360"
private const val SAMPLE_IMAGE_3 = "https://picsum.photos/seed/nmp3/640/360"

private fun labelFor(componentId: String): String = when (componentId) {
    "content-core" -> "ContentTreeWire (flat arena)"
    "content-view" -> "NostrContentView — live mention resolution"
    "content-mention-chip" -> "NostrMentionChip — kernel-resolved profiles"
    "content-minimal" -> "NostrMinimalContentView"
    "content-media-grid" -> "NostrMediaGrid"
    "content-quote-card" -> "NostrQuoteCard — kernel-resolved author"
    else -> componentId
}
