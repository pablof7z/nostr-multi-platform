// Requires: compose-ui, compose-foundation, compose-material3,
// androidx.compose.material:material-icons-extended (for ExpandMore,
// HelpOutline, PlayArrow, VolumeUp). Pulls `compose/content-media-grid` and
// `compose/content-kind-registry` as registry deps. Kotlin 1.9+.
//
// Compose mirror of the SwiftUI `NostrContentView`. Walks a
// `ContentTreeWire`, flattens the arena into block-level groups via
// `nostrContentGroups`, and renders each block (paragraph / heading / media /
// code / list / quote / rule / image / event-ref / placeholder).
//
// Data injection contract:
//   - Theming + tap callbacks come from `LocalNostrContentRenderer`
//     (see `compose/content-core`).
//   - Mention display labels are provided by the app via `mentionLabel`.
//   - Embedded events (`nostr:nevent…` / `nostr:naddr…`) render via the
//     kind-dispatch registry (ADR-0072): `EventRefBlock` delegates to the
//     `EmbeddedEvent` composable from `compose/content-kind-registry`, which
//     resolves the URI, reads the kernel-resolved `EmbeddedEventEnvelope` from
//     `LocalResolvedEventEmbeds`, and dispatches to the per-kind renderer in
//     `LocalNostrKindRegistry`.
//
// Inline runs are flattened into a single `AnnotatedString` (with per-run
// styling) and shown as a `ClickableText` so tap-offset → annotation routing
// can dispatch the matching callback. Block nodes use `Column`.

package org.nmp.gallery.registry

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.ClickableText
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

@Composable
public fun NostrContentView(
    tree: ContentTreeWire,
    modifier: Modifier = Modifier,
    textStyle: TextStyle = MaterialTheme.typography.bodyLarge,
    mentionLabel: (WireNostrUri) -> String = ::defaultMentionLabel,
) {
    val groups = nostrContentGroups(tree)
    if (groups.isEmpty()) return

    Column(
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = modifier,
    ) {
        for (group in groups) {
            RenderGroup(
                group = group,
                tree = tree,
                textStyle = textStyle,
                mentionLabel = mentionLabel,
            )
        }
    }
}

public fun defaultMentionLabel(uri: WireNostrUri): String {
    val value = uri.primaryId
    if (value.length <= 12) return value
    return "${value.take(8)}…${value.takeLast(4)}"
}

// ---------------------------------------------------------------------------
// Block dispatch
// ---------------------------------------------------------------------------

@Composable
private fun RenderGroup(
    group: NostrContentGroup,
    tree: ContentTreeWire,
    textStyle: TextStyle,
    mentionLabel: (WireNostrUri) -> String,
) {
    when (group) {
        is NostrContentGroup.Inline -> InlineGroup(
            level = group.level,
            children = group.children,
            tree = tree,
            textStyle = textStyle,
            mentionLabel = mentionLabel,
        )
        is NostrContentGroup.MediaGroup -> MediaGroupBlock(
            urls = group.urls,
            kind = group.kind,
        )
        is NostrContentGroup.EventRefGroup -> EventRefBlock(
            uri = group.uri,
        )
        is NostrContentGroup.CodeBlockGroup -> CodeBlockBlock(
            info = group.info,
            body = group.body,
        )
        is NostrContentGroup.BlockQuoteGroup -> BlockQuoteBlock(
            children = group.children,
            tree = tree,
            textStyle = textStyle,
            mentionLabel = mentionLabel,
        )
        is NostrContentGroup.ListGroup -> ListBlock(
            orderedStart = group.orderedStart,
            items = group.items,
            tree = tree,
            textStyle = textStyle,
            mentionLabel = mentionLabel,
        )
        NostrContentGroup.RuleGroup -> RuleBlock()
        is NostrContentGroup.ImageGroup -> ImageBlock(
            alt = group.alt,
            title = group.title,
            src = group.src,
        )
        is NostrContentGroup.PlaceholderGroup -> PlaceholderChip(reason = group.reason)
    }
}

// ---------------------------------------------------------------------------
// Inline rendering (Text + Text concatenation → AnnotatedString)
// ---------------------------------------------------------------------------

@Composable
private fun InlineGroup(
    level: NostrContentInlineLevel,
    children: List<UInt>,
    tree: ContentTreeWire,
    textStyle: TextStyle,
    mentionLabel: (WireNostrUri) -> String,
) {
    val renderer = LocalNostrContentRenderer.current
    val annotated = buildAnnotatedString {
        for (child in children) {
            appendInline(
                index = child,
                tree = tree,
                renderer = renderer,
                mentionLabel = mentionLabel,
            )
        }
    }
    val effectiveStyle = when (level) {
        NostrContentInlineLevel.Paragraph -> textStyle.copy(color = renderer.textColor)
        is NostrContentInlineLevel.Heading -> headingStyle(level.level).copy(color = renderer.textColor)
    }
    ClickableText(
        text = annotated,
        style = effectiveStyle,
        modifier = Modifier.fillMaxWidth(),
        onClick = { offset ->
            dispatchInlineTap(annotated, offset, renderer)
        },
    )
}

/**
 * Append one arena node's inline projection to the [AnnotatedString] builder.
 * Recursive children (emphasis / strong / link / heading / paragraph) are
 * walked here so the whole inline subtree collapses into the running text.
 * Block-level nodes (`list`, `code_block`, `rule`, `media`, `placeholder`)
 * appearing inside an inline group emit nothing rather than break flow.
 */
internal fun AnnotatedString.Builder.appendInline(
    index: UInt,
    tree: ContentTreeWire,
    renderer: NostrContentRenderer,
    mentionLabel: (WireNostrUri) -> String,
) {
    if (index == NOSTR_CONTENT_NEWLINE_SENTINEL) {
        append('\n')
        return
    }
    val node = tree.nodeAt(index) ?: return
    when (node) {
        is WireNode.Text -> append(node.text)
        is WireNode.Mention -> {
            val label = mentionLabel(node.uri)
            withAnnotationScope(MENTION_ANNOTATION, node.uri.primaryId) {
                withStyleScope(
                    SpanStyle(
                        color = renderer.mentionColor,
                        fontWeight = FontWeight.Bold,
                    ),
                ) { append("@$label") }
            }
        }
        is WireNode.EventRef -> {
            val short = shortEntity(node.uri.primaryId)
            withAnnotationScope(EVENT_REF_ANNOTATION, node.uri.primaryId) {
                withStyleScope(
                    SpanStyle(
                        color = renderer.linkColor,
                        fontWeight = FontWeight.Bold,
                    ),
                ) { append("↩ $short") }
            }
        }
        is WireNode.Hashtag -> {
            withAnnotationScope(HASHTAG_ANNOTATION, node.tag) {
                withStyleScope(
                    SpanStyle(
                        color = renderer.hashtagColor,
                        fontWeight = FontWeight.Bold,
                    ),
                ) { append("#${node.tag}") }
            }
        }
        is WireNode.Url -> {
            withAnnotationScope(LINK_ANNOTATION, node.url) {
                withStyleScope(SpanStyle(color = renderer.linkColor)) {
                    append(node.url)
                }
            }
        }
        is WireNode.Emoji -> append(":${node.shortcode}:")
        is WireNode.Invoice -> {
            withStyleScope(SpanStyle(color = renderer.linkColor)) {
                append("⚡ invoice")
            }
        }
        is WireNode.Emphasis -> withStyleScope(SpanStyle(fontStyle = FontStyle.Italic)) {
            for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
        }
        is WireNode.Strong -> withStyleScope(SpanStyle(fontWeight = FontWeight.Bold)) {
            for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
        }
        is WireNode.InlineCode -> withStyleScope(SpanStyle(fontFamily = FontFamily.Monospace)) {
            append(node.code)
        }
        is WireNode.Link -> {
            val href = node.href
            if (!href.isNullOrEmpty()) {
                withAnnotationScope(LINK_ANNOTATION, href) {
                    withStyleScope(
                        SpanStyle(
                            color = renderer.linkColor,
                            textDecoration = TextDecoration.Underline,
                        ),
                    ) {
                        for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
                    }
                }
            } else {
                for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
            }
        }
        is WireNode.Image -> {
            val alt = node.alt
            withStyleScope(SpanStyle(color = renderer.placeholderColor)) {
                append(if (alt.isEmpty()) "[image]" else "[$alt]")
            }
        }
        WireNode.SoftBreak -> append(' ')
        WireNode.HardBreak -> append('\n')
        is WireNode.Paragraph -> for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
        is WireNode.Heading -> for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
        is WireNode.BlockQuote -> for (child in node.children) appendInline(child, tree, renderer, mentionLabel)
        is WireNode.ListNode,
        is WireNode.CodeBlock,
        WireNode.Rule,
        is WireNode.Media,
        is WireNode.Placeholder -> { /* block-level — never inside an inline reduce */ }
    }
}

/** Resolve a tap offset against the annotations attached during inline build. */
private fun dispatchInlineTap(
    annotated: AnnotatedString,
    offset: Int,
    renderer: NostrContentRenderer,
) {
    annotated.getStringAnnotations(MENTION_ANNOTATION, offset, offset).firstOrNull()?.let {
        renderer.callbacks.onMentionTap(it.item)
        return
    }
    annotated.getStringAnnotations(EVENT_REF_ANNOTATION, offset, offset).firstOrNull()?.let {
        renderer.callbacks.onEventRefTap(it.item)
        return
    }
    annotated.getStringAnnotations(HASHTAG_ANNOTATION, offset, offset).firstOrNull()?.let {
        renderer.callbacks.onHashtagTap(it.item)
        return
    }
    annotated.getStringAnnotations(LINK_ANNOTATION, offset, offset).firstOrNull()?.let {
        renderer.callbacks.onLinkTap(it.item)
    }
}

// Annotation tags used to round-trip per-run identifiers from
// AnnotatedString into the tap-offset dispatcher. Centralized so the
// builder and the dispatcher cannot drift.
private const val MENTION_ANNOTATION = "nmp:mention"
private const val EVENT_REF_ANNOTATION = "nmp:event_ref"
private const val HASHTAG_ANNOTATION = "nmp:hashtag"
private const val LINK_ANNOTATION = "nmp:link"

/** Push a [SpanStyle] for the duration of [block] then pop it. Avoids the
 *  `androidx.compose.ui.text.withStyle` import to keep this file's API
 *  surface explicit. */
private inline fun AnnotatedString.Builder.withStyleScope(
    style: SpanStyle,
    block: AnnotatedString.Builder.() -> Unit,
) {
    val index = pushStyle(style)
    try {
        block()
    } finally {
        pop(index)
    }
}

/** Push a string annotation for the duration of [block] then pop it. */
private inline fun AnnotatedString.Builder.withAnnotationScope(
    tag: String,
    annotation: String,
    block: AnnotatedString.Builder.() -> Unit,
) {
    val index = pushStringAnnotation(tag, annotation)
    try {
        block()
    } finally {
        pop(index)
    }
}

private fun headingStyle(level: UByte): TextStyle {
    return when (level.toInt()) {
        1 -> TextStyle(fontSize = 30.sp, fontWeight = FontWeight.Bold)
        2 -> TextStyle(fontSize = 26.sp, fontWeight = FontWeight.Bold)
        3 -> TextStyle(fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
        4 -> TextStyle(fontSize = 19.sp, fontWeight = FontWeight.SemiBold)
        5 -> TextStyle(fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
        else -> TextStyle(fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
    }
}

private fun shortEntity(value: String): String {
    if (value.length <= 12) return value
    return "${value.take(8)}…${value.takeLast(4)}"
}
