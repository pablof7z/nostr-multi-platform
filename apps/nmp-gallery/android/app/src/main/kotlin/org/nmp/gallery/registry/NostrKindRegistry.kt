// Requires: compose-ui, compose-foundation, compose-material3. Kotlin 1.9+.
// Depends on `compose/content-core` and `compose/user-avatar`.
//
// Single source of truth for kind → Compose renderer dispatch on Android.
// Compose mirror of the SwiftUI `NostrKindRegistry.swift` and the TUI
// `NostrKindRegistry`.
//
// THIN-SHELL (D0): performs NO protocol parsing and NO kind *classification*.
// The kernel already resolved each embedded event into a typed
// [EmbedKindProjection] (exactly one variant non-null); the registry only picks
// the matching renderer from that already-typed variant. Adding a new embed
// kind means adding a variant to the kernel's resolver + a renderer here —
// never a `when (kind)` over a raw integer in Kotlin.

package org.nmp.gallery.registry

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/**
 * Renderer for one [EmbedKindProjection] variant. Mirrors the SwiftUI
 * `KindRenderer` protocol and the TUI `KindRenderer` trait — the body is the
 * Compose UI emitted for the projection. `registry` is provided so recursive
 * kind dispatch is possible.
 */
public fun interface KindRenderer {
    @Composable
    public fun Render(projection: EmbedKindProjection, registry: NostrKindRegistry)
}

/**
 * Kind → Compose renderer dispatch table. Built via the `set*` setters or
 * [registerUnknown]; consulted by [EmbeddedEvent]. Mirrors the SwiftUI
 * `NostrKindRegistry` shape so apps porting renderers can do so 1:1.
 */
public class NostrKindRegistry(
    private val fallback: KindRenderer = DefaultUnknownRenderer,
) {
    private var shortNote: KindRenderer? = null
    private var article: KindRenderer? = null
    private var highlight: KindRenderer? = null
    private var profile: KindRenderer? = null
    private val unknownByKind: MutableMap<Int, KindRenderer> = mutableMapOf()

    public fun setShortNote(renderer: KindRenderer) {
        shortNote = renderer
    }

    public fun setArticle(renderer: KindRenderer) {
        article = renderer
    }

    public fun setHighlight(renderer: KindRenderer) {
        highlight = renderer
    }

    public fun setProfile(renderer: KindRenderer) {
        profile = renderer
    }

    public fun registerUnknown(kind: Int, renderer: KindRenderer) {
        unknownByKind[kind] = renderer
    }

    /** Resolve the renderer responsible for a projection (mirrors SwiftUI). */
    public fun resolve(projection: EmbedKindProjection): KindRenderer = when {
        projection.shortNote != null -> shortNote ?: fallback
        projection.article != null -> article ?: fallback
        projection.highlight != null -> highlight ?: fallback
        projection.profile != null -> profile ?: fallback
        projection.unknown != null -> unknownByKind[projection.unknown.kind] ?: fallback
        else -> fallback
    }

    public companion object {
        /**
         * Returns a registry pre-populated with the built-in defaults for every
         * known projection variant. Replace any slot via `setArticle(...)` to
         * swap in a richer handler installed from `compose/content-kind-30023`.
         */
        public fun makeDefault(): NostrKindRegistry = NostrKindRegistry().apply {
            setShortNote(DefaultShortNoteRenderer)
            setArticle(DefaultArticleRenderer)
            setHighlight(DefaultHighlightRenderer)
            setProfile(DefaultProfileRenderer)
        }
    }
}

// ---------------------------------------------------------------------------
// Default renderers — Android peers of the SwiftUI `Default*Renderer` structs.
// ---------------------------------------------------------------------------

/** Default short-note renderer. Byline + plain-text content. */
public val DefaultShortNoteRenderer: KindRenderer = KindRenderer { projection, _ ->
    val note = projection.shortNote ?: return@KindRenderer
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = note.authorPubkey,
            caption = "note",
            avatarConsumerId = "embed-note-${note.id}",
        )
        if (note.content.isNotEmpty()) {
            Text(note.content, style = MaterialTheme.typography.bodyMedium)
        }
        note.mediaUrls.forEach { url ->
            Text(
                url,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

/** Default article renderer. Title · byline · summary. */
public val DefaultArticleRenderer: KindRenderer = KindRenderer { projection, _ ->
    val article = projection.article ?: return@KindRenderer
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = article.authorPubkey,
            caption = "article",
            avatarConsumerId = "embed-article-${article.id}",
        )
        article.title?.takeIf { it.isNotEmpty() }?.let { title ->
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
        val body = article.summary?.takeIf { it.isNotEmpty() }
            ?: article.content.takeIf { it.isNotEmpty() }
        body?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Default highlight renderer. Pull-quote + byline + optional source. */
public val DefaultHighlightRenderer: KindRenderer = KindRenderer { projection, _ ->
    val highlight = projection.highlight ?: return@KindRenderer
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = highlight.authorPubkey,
            caption = "highlight",
            avatarConsumerId = "embed-highlight-${highlight.id}",
        )
        Text(
            "“${highlight.highlightedText}”",
            style = MaterialTheme.typography.bodyLarge.copy(fontStyle = FontStyle.Italic),
            modifier = Modifier.padding(start = 6.dp),
        )
        highlight.sourceUrl?.takeIf { it.isNotEmpty() }?.let { url ->
            Text(url, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
        }
    }
}

/** Default profile renderer. Byline + about line. */
public val DefaultProfileRenderer: KindRenderer = KindRenderer { projection, _ ->
    val profile = projection.profile ?: return@KindRenderer
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = profile.pubkey,
            caption = profile.nip05?.takeIf { it.isNotEmpty() } ?: "profile",
            avatarConsumerId = "embed-profile-${profile.pubkey}",
        )
        profile.about?.takeIf { it.isNotEmpty() }?.let { about ->
            Text(about, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Fallback renderer for [UnknownProjection] — numeric kinds without a handler. */
public val DefaultUnknownRenderer: KindRenderer = KindRenderer { projection, _ ->
    val unknown = projection.unknown ?: return@KindRenderer
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = unknown.authorPubkey,
            caption = "kind ${unknown.kind}",
            avatarConsumerId = "embed-unknown-${unknown.authorPubkey}-${unknown.kind}",
        )
        val body = unknown.altText?.takeIf { it.isNotEmpty() }
            ?: unknown.content.takeIf { it.isNotEmpty() }
        body?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

// ---------------------------------------------------------------------------
// Shared byline header for the default renderers. Paints the author avatar
// (which self-claims the kind:0 via the profile host wired into
// `compose/user-avatar`) plus a reactively-resolved display name
// ([NostrProfileName], which self-claims the same kind:0) and a kind/time
// caption. No author display data rides the embed projection — display joins
// reactively at this presentation layer (display-separation doctrine).
// ---------------------------------------------------------------------------

@Composable
internal fun EmbedByline(
    authorPubkey: String,
    caption: String,
    avatarConsumerId: String,
) {
    androidx.compose.foundation.layout.Row(
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        NostrAvatar(
            pubkey = authorPubkey,
            size = 28.dp,
            consumerId = avatarConsumerId,
        )
        androidx.compose.foundation.layout.Spacer(Modifier.padding(start = 8.dp))
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            NostrProfileName(
                pubkey = authorPubkey,
                style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.Bold),
                consumerId = "$avatarConsumerId-name",
            )
            Text(
                caption,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
