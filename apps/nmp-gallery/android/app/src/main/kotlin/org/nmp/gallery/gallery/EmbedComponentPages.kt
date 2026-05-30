package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.ClaimedEventWire
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.bridge.GalleryShowcaseReferences
import org.nmp.gallery.registry.ContentTreeWire
import org.nmp.gallery.registry.LocalNostrContentRenderer
import org.nmp.gallery.registry.NostrContentRenderer
import org.nmp.gallery.registry.NostrContentView
import org.nmp.gallery.registry.NostrQuoteCardModel
import org.nmp.gallery.registry.ProfileWire
import org.nmp.gallery.registry.WireNode
import org.nmp.gallery.registry.WireNostrUri
import org.nmp.gallery.registry.WireNostrUriKind
import org.nmp.gallery.registry.defaultMentionLabel

/**
 * Showcase pages for the kind-dispatch embed renderers (ADR-0034 / M16).
 *
 * Each page builds a [ContentTreeWire] of surrounding prose plus an
 * `EventRef` (or `Mention`) for a real bech32 URI, then renders it through
 * [NostrContentView] — exactly the iOS `EmbedComponentPages.swift` shape. On
 * hitting the `EventRef`, `NostrContentView` calls the page's
 * `quoteCardProvider`, which maps a resolved `claimedEvents[primaryId]` entry
 * to a [NostrQuoteCardModel]. The `DisposableEffect` lifecycle fires the
 * `claim` on the URI so the kernel resolves the event (cache or relay) and
 * surfaces it in `projections.claimed_events`; recomposition then paints the
 * inline card mid-prose: "this is a great point [card] what do you think?".
 *
 * Profile embeds resolve as an inline `@DisplayName` mention via the
 * `mentionLabel` callback (kind:0 path), claimed through `claimProfile` — no
 * event claim is required for `npub:` URIs.
 *
 * Mirrors the TUI showcase in `apps/nmp-gallery/tui/src/data.rs::from_live`.
 *
 * Article (kind:30023) and highlight (kind:9802) embeds flow through the same
 * `EventRef` → `NostrQuoteCard` path: Android's shared `NostrContentView`
 * has no per-kind dispatch (every `EventRef` renders as a quote card), so they
 * render as generic quote cards rather than the typed article/highlight
 * projections iOS paints via the kind registry. Inline surrounding text is
 * identical across all four; the typed-projection inline renderer is an
 * Android gap tracked separately.
 */

@Composable
fun EmbedComponentPage(
    model: GalleryModel,
    componentId: String,
) {
    val claimedEvents by model.claimedEvents.collectAsState()
    val profileMap by model.profileMap.collectAsState()
    val showcase = model.showcase

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
            EmbedComponentBody(
                componentId = componentId,
                showcase = showcase,
                claimedEvents = claimedEvents,
                profileMap = profileMap,
                model = model,
            )
        }
    }
}

@Composable
private fun EmbedComponentBody(
    componentId: String,
    showcase: GalleryShowcaseReferences,
    claimedEvents: Map<String, ClaimedEventWire>,
    profileMap: Map<String, ProfileWire>,
    model: GalleryModel,
) {
    when (componentId) {
        "embed-article" -> ArticleEmbedPage(showcase, claimedEvents, profileMap, model)
        "embed-profile" -> ProfileEmbedPage(showcase, profileMap, model)
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
    profileMap: Map<String, ProfileWire>,
    model: GalleryModel,
) {
    val articleUri = showcase.article.uri
    // The article author (Gigi) is a DIFFERENT pubkey than the showcase
    // profile (pablof7z), and Android has no per-kind inline renderer — the
    // EventRef paints as a generic quote card, so nothing instantiates a
    // NostrProfileName/avatar that would claim the author's kind:0 (the way
    // iOS's typed ArticleEmbed does). Without an explicit claim the kernel
    // never fetches Gigi's kind:0, `claimed_events` enrichment leaves
    // `author_display_name` null, and the byline falls back to hex.
    //
    // Component-owned claiming (ADR-0034; mirrors iOS #847 / ProfileEmbedPage
    // below): the presentation component claims the author's profile so the
    // kernel resolves it into its profile cache — the kernel must NEVER fetch
    // an author's kind:0 as a side effect of event ingest. The author hex is
    // the pubkey TLV of the naddr, available verbatim as the middle field of
    // the addressable coordinate `<kind>:<pubkey>:<d-tag>`.
    val articleAuthorPubkey = articleAuthorPubkeyOf(showcase)

    DisposableEffect(articleUri) {
        model.claimEvent(articleUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(articleUri, GalleryModel.CONSUMER_ID)
        }
    }

    DisposableEffect(articleAuthorPubkey) {
        if (articleAuthorPubkey != null) {
            model.claimProfile(articleAuthorPubkey, GalleryModel.CONSUMER_ID)
        }
        onDispose {
            if (articleAuthorPubkey != null) {
                model.releaseProfile(articleAuthorPubkey, GalleryModel.CONSUMER_ID)
            }
        }
    }

    // Arena:
    //   0  text "hey, check out my article "
    //   1  eventRef(article naddr)
    //   2  text " I hope you enjoy it!"
    //   3  paragraph([0, 1, 2])
    val tree = ContentTreeWire(
        nodes = listOf(
            WireNode.Text("hey, check out my article "),
            WireNode.EventRef(articleRefUri(showcase)),
            WireNode.Text(" I hope you enjoy it!"),
            WireNode.Paragraph(children = listOf(0u, 1u, 2u)),
        ),
        roots = listOf(3u),
    )

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Article embed — kind:30023 via NostrKindRegistry",
            style = MaterialTheme.typography.bodySmall,
        )
        NostrContentView(
            tree = tree,
            // Resolve the byline from the live profile map as well as the
            // claimed_events enrichment — the same read-from-profile shape the
            // note path uses (ContentComponentPages QuoteCardShowcase) and that
            // iOS's typed renderer relies on. The claimed_events enrichment also
            // resolves Gigi (kernel `profile_for_pubkey` reads the general
            // profile cache the claim populates), but reading profileMap makes
            // the byline robust to enrichment-tick timing.
            quoteCardProvider = { uri -> quoteCardFor(uri, claimedEvents, profileMap) },
        )
        Text(
            "The renderer fires `claim` on the article naddr and on the author's kind:0; the kernel resolves kind:30023 and the author profile (Gigi) so the byline resolves. Android renders it inline as a quote card (no per-kind inline dispatch yet).",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── Profile — inline npub mention chip ────────────────────────────────────────────

@Composable
private fun ProfileEmbedPage(
    showcase: GalleryShowcaseReferences,
    profileMap: Map<String, ProfileWire>,
    model: GalleryModel,
) {
    val pubkeyHex = showcase.profile.pubkeyHex

    // A hidden claim owns the showcase pubkey while this page is visible —
    // mirrors the real-app pattern where a parent note row or profile header
    // would own the claim, and iOS's hidden-avatar claim.
    DisposableEffect(pubkeyHex) {
        model.claimProfile(pubkeyHex, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseProfile(pubkeyHex, GalleryModel.CONSUMER_ID)
        }
    }

    // Arena:
    //   0  text "met "
    //   1  mention(SHOWCASE_PUBKEY_HEX)
    //   2  text " at a nostr conference last week, brilliant mind"
    //   3  paragraph([0, 1, 2])
    val tree = ContentTreeWire(
        nodes = listOf(
            WireNode.Text("met "),
            WireNode.Mention(profileMentionUri(showcase)),
            WireNode.Text(" at a nostr conference last week, brilliant mind"),
            WireNode.Paragraph(children = listOf(0u, 1u, 2u)),
        ),
        roots = listOf(3u),
    )

    val mentionLabel: (WireNostrUri) -> String = { uri ->
        profileMap[uri.primaryId]?.displayName ?: defaultMentionLabel(uri)
    }

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Inline profile mention — kind:0 via mention chip",
            style = MaterialTheme.typography.bodySmall,
        )
        NostrContentView(tree = tree, mentionLabel = mentionLabel)
        Text(
            "Profile mentions resolve via projections.claimed_profiles → resolved_profiles — the same kind:0 path the user-* pages use. No embed claim is required for `npub:` URIs.",
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

    DisposableEffect(noteUri) {
        model.claimEvent(noteUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(noteUri, GalleryModel.CONSUMER_ID)
        }
    }

    // Arena:
    //   0  text "this is a great point "
    //   1  eventRef(nevent)
    //   2  text " what do you think?"
    //   3  paragraph([0, 1, 2])
    val tree = ContentTreeWire(
        nodes = listOf(
            WireNode.Text("this is a great point "),
            WireNode.EventRef(noteRefUri(showcase)),
            WireNode.Text(" what do you think?"),
            WireNode.Paragraph(children = listOf(0u, 1u, 2u)),
        ),
        roots = listOf(3u),
    )

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Note embed — kind:1 via NostrKindRegistry",
            style = MaterialTheme.typography.bodySmall,
        )
        NostrContentView(
            tree = tree,
            quoteCardProvider = { uri -> quoteCardFor(uri, claimedEvents) },
        )
        Text(
            "nevent1… URIs resolve via the same `claim_event` path. The default short-note renderer paints author + content inline between the surrounding prose.",
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

    DisposableEffect(highlightUri) {
        model.claimEvent(highlightUri, GalleryModel.CONSUMER_ID)
        onDispose {
            model.releaseEvent(highlightUri, GalleryModel.CONSUMER_ID)
        }
    }

    // Arena:
    //   0  text "found this interesting "
    //   1  eventRef(highlight nevent)
    //   2  paragraph([0, 1])
    val tree = ContentTreeWire(
        nodes = listOf(
            WireNode.Text("found this interesting "),
            WireNode.EventRef(highlightRefUri(showcase)),
            WireNode.Paragraph(children = listOf(0u, 1u)),
        ),
        roots = listOf(2u),
    )

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Highlight embed — kind:9802 via HighlightEmbed renderer",
            style = MaterialTheme.typography.bodySmall,
        )
        NostrContentView(
            tree = tree,
            quoteCardProvider = { uri -> quoteCardFor(uri, claimedEvents) },
        )
        Text(
            "NIP-84 highlights render as a pull-quote with optional source link. The kernel resolves kind:9802; iOS paints the typed projection, while Android renders it inline as a quote card (no per-kind inline dispatch yet).",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// ── URI + quote-card helpers ────────────────────────────────────────────
//
// These mirror the private helpers in `ContentComponentPages.kt`. Kotlin
// `private` is file-scoped, not package-scoped, so the pattern is duplicated
// here rather than shared.

private fun articleRefUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = showcase.article.uri,
    kind = WireNostrUriKind.Address,
    // The `primaryId` must match the kernel-emitted `claimed_events` key
    // exactly so `quoteCardProvider`'s `claimedEvents[uri.primaryId]` lookup
    // hits. For an naddr that is the `<kind>:<pubkey>:<d>` coordinate, already
    // computed into the showcase references on master.
    primaryId = showcase.article.primaryId,
)

private fun noteRefUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = showcase.note.uri,
    kind = WireNostrUriKind.Event,
    primaryId = showcase.note.primaryId,
)

private fun highlightRefUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = showcase.highlight.uri,
    kind = WireNostrUriKind.Event,
    primaryId = showcase.highlight.primaryId,
)

private fun profileMentionUri(showcase: GalleryShowcaseReferences) = WireNostrUri(
    uri = "nostr:${showcase.profile.npub}",
    kind = WireNostrUriKind.Profile,
    primaryId = showcase.profile.pubkeyHex,
)

private fun quoteCardFor(
    uri: WireNostrUri,
    claimedEvents: Map<String, ClaimedEventWire>,
    profileMap: Map<String, ProfileWire> = emptyMap(),
): NostrQuoteCardModel? {
    val event = claimedEvents[uri.primaryId] ?: return null
    // Prefer the kernel's claimed_events enrichment; fall back to a separately
    // claimed profile in the live profile map. Mirrors ContentComponentPages
    // QuoteCardShowcase (the note path) and iOS's read-from-profile byline.
    val profile = profileMap[event.authorPubkey]
    return NostrQuoteCardModel(
        id = event.id,
        unresolvedUri = uri.uri,
        authorPubkey = event.authorPubkey,
        authorDisplayName = event.authorDisplayName ?: profile?.displayName,
        authorAvatarUrl = event.authorPictureUrl ?: profile?.pictureUrl,
        content = event.content,
        createdAtDisplay = event.createdAt.takeIf { it > 0L }?.toString(),
    )
}

/**
 * The article author's hex pubkey, parsed from the addressable coordinate
 * `<kind>:<pubkey>:<d-tag>` (the naddr's pubkey TLV). `kind` and `pubkey`
 * never contain `:`, so index `[1]` is the author even when the d-tag does.
 * Returns null if the coordinate is malformed (the claim is then skipped).
 */
private fun articleAuthorPubkeyOf(showcase: GalleryShowcaseReferences): String? =
    showcase.article.primaryId.split(":").getOrNull(1)?.takeIf { it.isNotBlank() }

private fun labelFor(componentId: String): String = when (componentId) {
    "embed-article" -> "Article embed — kind:30023"
    "embed-profile" -> "Profile embed — kind:0"
    "embed-note" -> "Note embed — kind:1"
    "embed-highlight" -> "Highlight embed — kind:9802"
    else -> componentId
}
