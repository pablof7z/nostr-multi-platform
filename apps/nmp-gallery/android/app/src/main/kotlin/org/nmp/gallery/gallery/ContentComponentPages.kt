package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.bridge.GalleryShowcaseReferences
import org.nmp.gallery.bridge.ResolvedEventEnvelopeWire
import org.nmp.gallery.bridge.projectionContentText
import org.nmp.gallery.bridge.projectionLong
import org.nmp.gallery.bridge.projectionString
import org.nmp.gallery.bridge.projectionStrings
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
import org.nmp.gallery.registry.NostrRelativeTime
import org.nmp.gallery.registry.ProfileWire
import org.nmp.gallery.registry.WireNode
import org.nmp.gallery.registry.WireNostrUri
import org.nmp.gallery.registry.WireNostrUriKind
import org.nmp.gallery.registry.defaultMentionLabel

@Composable
fun ContentComponentPage(model: GalleryModel, componentId: String) {
    val profileMap by model.profileMap.collectAsState()
    val resolvedEventEmbeds by model.resolvedEventEmbeds.collectAsState()
    val showcase = model.showcase
    var rawMode by remember { mutableStateOf(false) }

    DisposableEffect(showcase) {
        model.resolveProfileRef(showcase.profile.pubkeyHex, GalleryModel.CONSUMER_ID)
        model.resolveEventRef(showcase.note.uri, GalleryModel.CONSUMER_ID)
        model.resolveEventRef(showcase.article.uri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseProfileRef(showcase.profile.pubkeyHex, GalleryModel.CONSUMER_ID)
            model.releaseEventRef(showcase.note.uri, GalleryModel.CONSUMER_ID)
            model.releaseEventRef(showcase.article.uri, GalleryModel.CONSUMER_ID)
        }
    }

    val renderer = NostrContentRenderer(
        textColor = MaterialTheme.colorScheme.onSurface,
        secondaryTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
        mentionColor = MaterialTheme.colorScheme.primary,
        hashtagColor = MaterialTheme.colorScheme.tertiary,
        linkColor = MaterialTheme.colorScheme.primary,
    )

    val mentionLabel: (WireNostrUri) -> String = { uri ->
        if (rawMode) uri.uri
        else profileMap[uri.primaryId]?.displayName ?: defaultMentionLabel(uri)
    }

    val showsRawToggle = componentId in setOf(
        "content-view", "content-mention-chip", "content-minimal",
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
            if (showsRawToggle) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = if (rawMode) "Raw wire" else "Rendered",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Switch(checked = rawMode, onCheckedChange = { rawMode = it })
                }
            }
            ContentComponentBody(
                componentId = componentId,
                showcase = showcase,
                profileMap = profileMap,
                resolvedEventEmbeds = resolvedEventEmbeds,
                mentionLabel = mentionLabel,
            )
        }
    }
}

@Composable
private fun ContentComponentBody(
    componentId: String,
    showcase: GalleryShowcaseReferences,
    profileMap: Map<String, ProfileWire>,
    resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>,
    mentionLabel: (WireNostrUri) -> String,
) {
    when (componentId) {
        "content-core" -> ContentCoreShowcase(showcase, mentionLabel)
        "content-view" -> ContentViewShowcase(showcase, mentionLabel, resolvedEventEmbeds)
        "content-mention-chip" -> MentionChipShowcase(showcase, profileMap, mentionLabel)
        "content-minimal" -> MinimalContentShowcase(showcase, mentionLabel)
        "content-media-grid" -> MediaGridShowcase(resolvedEventEmbeds)
        "content-quote-card" -> QuoteCardShowcase(showcase, profileMap, resolvedEventEmbeds)
        else -> Text("Unknown content component: $componentId")
    }
}

@Composable
private fun ContentCoreShowcase(
    showcase: GalleryShowcaseReferences,
    mentionLabel: (WireNostrUri) -> String,
) {
    NostrContentView(tree = showcaseTextTree(showcase), mentionLabel = mentionLabel)
}

@Composable
private fun ContentViewShowcase(
    showcase: GalleryShowcaseReferences,
    mentionLabel: (WireNostrUri) -> String,
    resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>,
) {
    NostrContentView(
        tree = showcaseRichTree(showcase),
        mentionLabel = mentionLabel,
        quoteCardProvider = { uri -> quoteCardFor(uri, resolvedEventEmbeds) },
    )
}

@Composable
private fun MentionChipShowcase(
    showcase: GalleryShowcaseReferences,
    profileMap: Map<String, ProfileWire>,
    mentionLabel: (WireNostrUri) -> String,
) {
    val primary = profileMap[showcase.profile.pubkeyHex]
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("NostrContentView — inline mention", style = MaterialTheme.typography.bodySmall)
        NostrContentView(tree = showcaseMentionTree(showcase), mentionLabel = mentionLabel)
        Text("NostrMentionChip — live kernel-backed", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = showcase.profile.pubkeyHex,
            displayName = primary?.displayName,
            avatarUrl = primary?.pictureUrl,
        )
        Text("No avatar variant", style = MaterialTheme.typography.bodySmall)
        NostrMentionChip(
            pubkey = showcase.profile.pubkeyHex,
            displayName = primary?.displayName,
            showsAvatar = false,
        )
    }
}

@Composable
private fun MinimalContentShowcase(
    showcase: GalleryShowcaseReferences,
    mentionLabel: (WireNostrUri) -> String,
) {
    NostrContentView(tree = showcaseShortTree(showcase), mentionLabel = mentionLabel)
}

@Composable
private fun MediaGridShowcase(resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>) {
    val urls = mediaUrlsFromResolvedEventEmbeds(resolvedEventEmbeds)
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Relay-backed media", style = MaterialTheme.typography.bodySmall)
        if (urls.isEmpty()) {
            Text("Waiting for media from the resolved article.", style = MaterialTheme.typography.bodySmall)
        } else {
            NostrMediaGrid(imageUrls = urls)
        }
    }
}

@Composable
private fun QuoteCardShowcase(
    showcase: GalleryShowcaseReferences,
    profileMap: Map<String, ProfileWire>,
    resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>,
) {
    val noteUri = noteUri(showcase)
    val quoteModel = quoteCardFor(noteUri, resolvedEventEmbeds)?.let { model ->
        val profile = model.authorPubkey?.let { profileMap[it] }
        model.copy(
            authorDisplayName = profile?.displayName,
            authorAvatarUrl = profile?.pictureUrl,
        )
    } ?: NostrQuoteCardModel(id = showcase.note.primaryId, unresolvedUri = showcase.note.uri)
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Rich", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Rich)
        Text("Compact", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Compact)
        Text("Collapsed", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel, variant = NostrQuoteCardVariant.Collapsed)
        Text("Missing", style = MaterialTheme.typography.bodySmall)
        NostrQuoteCard(model = quoteModel.copy(unresolvedUri = showcase.note.uri), variant = NostrQuoteCardVariant.Missing)
    }
}

// ── Relay-reference content trees ────────────────────────────────────────────

private fun showcaseMentionTree(showcase: GalleryShowcaseReferences): ContentTreeWire {
    val nodes = listOf<WireNode>(
        WireNode.Text("Hey "),
        WireNode.Mention(
            uri = profileUri(showcase),
        ),
        WireNode.Paragraph(children = listOf(0u, 1u)),
    )
    return ContentTreeWire(nodes = nodes, roots = listOf(2u))
}

private fun showcaseTextTree(showcase: GalleryShowcaseReferences): ContentTreeWire {
    val nodes = listOf<WireNode>(
        WireNode.EventRef(noteUri(showcase)),
        WireNode.Paragraph(children = listOf(0u)),
    )
    return ContentTreeWire(nodes = nodes, roots = listOf(1u))
}

private fun showcaseShortTree(showcase: GalleryShowcaseReferences): ContentTreeWire {
    val nodes = listOf<WireNode>(
        WireNode.Mention(profileUri(showcase)),
        WireNode.Paragraph(children = listOf(0u)),
    )
    return ContentTreeWire(nodes = nodes, roots = listOf(1u))
}

private fun showcaseRichTree(showcase: GalleryShowcaseReferences): ContentTreeWire {
    val nodes = listOf<WireNode>(
        // 0
        WireNode.Text("Relay note "),
        // 1
        WireNode.Mention(
            uri = profileUri(showcase),
        ),
        // 2
        WireNode.Text(", "),
        // 3
        WireNode.EventRef(noteUri(showcase)),
        // 4
        WireNode.Text(" "),
        // 5
        WireNode.Paragraph(children = listOf(0u, 1u, 2u, 3u, 4u)),
        // 6
    )
    return ContentTreeWire(
        nodes = nodes,
        roots = listOf(5u),
    )
}

private fun noteUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = showcase.note.uri,
    kind = WireNostrUriKind.Event,
    primaryId = showcase.note.primaryId,
)

private fun profileUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = "nostr:${showcase.profile.npub}",
    kind = WireNostrUriKind.Profile,
    primaryId = showcase.profile.pubkeyHex,
)

private fun quoteCardFor(
    uri: WireNostrUri,
    resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>,
): NostrQuoteCardModel? {
    val event = resolvedEventEmbeds[uri.primaryId] ?: return null
    return NostrQuoteCardModel(
        id = event.projectionString("id") ?: event.primaryId,
        unresolvedUri = uri.uri,
        authorPubkey = event.projectionString("authorPubkey"),
        content = event.projectionString("content") ?: event.projectionContentText(),
        mediaThumbnailUrl = mediaUrls(event).firstOrNull(),
        createdAtDisplay = event.projectionLong("createdAt")
            ?.takeIf { it > 0L }
            ?.let { NostrRelativeTime.ago(it) },
    )
}

private fun mediaUrlsFromResolvedEventEmbeds(
    resolvedEventEmbeds: Map<String, ResolvedEventEnvelopeWire>,
): List<String> = resolvedEventEmbeds.values.flatMap(::mediaUrls).distinct()

private fun mediaUrls(event: ResolvedEventEnvelopeWire): List<String> {
    val projected = event.projectionStrings("mediaUrls") +
        listOfNotNull(event.projectionString("heroImageUrl"))
    val inline = event.projectionString("content").orEmpty()
        .split(Regex("\\s+"))
        .filter(::looksLikeMedia)
    return (projected + inline).distinct()
}

private fun looksLikeMedia(value: String): Boolean {
    val lower = value.lowercase()
    return (lower.startsWith("http://") || lower.startsWith("https://")) &&
        listOf(".jpg", ".jpeg", ".png", ".gif", ".webp").any { lower.contains(it) }
}

private fun labelFor(componentId: String): String = when (componentId) {
    "content-core" -> "ContentTreeWire (flat arena)"
    "content-view" -> "NostrContentView — live mention resolution"
    "content-mention-chip" -> "NostrMentionChip — kernel-backed profiles"
    "content-minimal" -> "NostrMinimalContentView"
    "content-media-grid" -> "NostrMediaGrid"
    "content-quote-card" -> "NostrQuoteCard — kernel-backed author"
    else -> componentId
}
