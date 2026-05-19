package org.nmp.gallery.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import org.nmp.gallery.model.ContentTreeDto
import org.nmp.gallery.model.EmbedEntry
import org.nmp.gallery.model.RenderContext
import org.nmp.gallery.model.SegmentDto

/**
 * Compose port of Swift `SegmentDtoView`. Walks a [ContentTreeDto] segment
 * list, dispatching per [SegmentDto] variant exactly as the Rust doctrine
 * requires (content-rendering.md §5).
 *
 * Inline runs (`text` / `hashtag` / `url` / `emoji` / `invoice` / `mention`)
 * concatenate via `buildAnnotatedString` so word-wrap behaves naturally.
 * Runs that contain a resolved custom-emoji image fall through to
 * [FlowRow] so the [AsyncImage] can appear inline.
 *
 * Block segments (`media` / `eventRef` / `markdownBlock`) render as
 * separate Composables, with PD-015 depth + cycle guarding applied through
 * the [RenderContext] descend / shouldCollapse pair.
 */

/** Italic emphasis style propagated through markdown inline runs. */
internal val LocalEmphasis = compositionLocalOf { false }

/** Bold strong style propagated through markdown inline runs. */
internal val LocalStrong = compositionLocalOf { false }

@Composable
fun SegmentDtoView(
    tree: ContentTreeDto,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext = RenderContext(),
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (tree.segments.isEmpty()) {
            Text(
                "(empty content)",
                style = MaterialTheme.typography.labelSmall.copy(
                    fontStyle = FontStyle.Italic,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
        }
        for (group in groupSegments(tree.segments)) {
            when (group) {
                is SegmentGroup.Inline -> InlineRun(group.segments, embeds)
                is SegmentGroup.Block -> BlockSegment(group.segment, embeds, ctx)
            }
        }
    }
}

private sealed class SegmentGroup {
    data class Inline(val segments: List<SegmentDto>) : SegmentGroup()
    data class Block(val segment: SegmentDto) : SegmentGroup()
}

private fun groupSegments(segments: List<SegmentDto>): List<SegmentGroup> {
    val groups = mutableListOf<SegmentGroup>()
    val run = mutableListOf<SegmentDto>()
    for (seg in segments) {
        if (seg.isInline()) {
            run.add(seg)
        } else {
            if (run.isNotEmpty()) {
                groups.add(SegmentGroup.Inline(run.toList()))
                run.clear()
            }
            groups.add(SegmentGroup.Block(seg))
        }
    }
    if (run.isNotEmpty()) groups.add(SegmentGroup.Inline(run.toList()))
    return groups
}

private fun SegmentDto.isInline(): Boolean = when (this) {
    is SegmentDto.Text,
    is SegmentDto.Hashtag,
    is SegmentDto.Url,
    is SegmentDto.Emoji,
    is SegmentDto.Invoice,
    is SegmentDto.Mention -> true
    is SegmentDto.Media,
    is SegmentDto.EventRef,
    is SegmentDto.MarkdownBlock,
    is SegmentDto.Unknown -> false
}

private fun List<SegmentDto>.hasResolvedEmoji(): Boolean =
    any { it is SegmentDto.Emoji && it.url != null }

// MARK: - Inline run

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun InlineRun(
    segments: List<SegmentDto>,
    embeds: Map<String, EmbedEntry>,
) {
    if (segments.hasResolvedEmoji()) {
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            for (seg in segments) FlowItem(seg, embeds)
        }
    } else {
        val emphasis = LocalEmphasis.current
        val strong = LocalStrong.current
        val text = buildAnnotatedString {
            for (seg in segments) appendInline(seg, emphasis, strong)
        }
        // Honour the ambient text style so headings / link tints / size
        // overrides set by enclosing markdown nodes flow through.
        val base = LocalTextStyle.current
        val effective =
            if (base == androidx.compose.ui.text.TextStyle.Default) {
                MaterialTheme.typography.bodyMedium
            } else base
        Text(text, style = effective)
    }
}

/** Per-segment paint inside a [FlowRow] (used when emoji images appear). */
@Composable
private fun FlowItem(seg: SegmentDto, embeds: Map<String, EmbedEntry>) {
    when (seg) {
        is SegmentDto.Text -> Text(seg.text, style = MaterialTheme.typography.bodyMedium)
        is SegmentDto.Hashtag -> Text(
            "#${seg.tag}",
            style = MaterialTheme.typography.bodyMedium,
            color = SwiftAccent,
            fontWeight = FontWeight.Bold,
        )
        is SegmentDto.Url -> Text(
            seg.url,
            style = MaterialTheme.typography.bodyMedium,
            color = SwiftBlue,
        )
        is SegmentDto.Invoice -> Text(
            "⚡ ${seg.value.take(12)}…",
            style = MaterialTheme.typography.bodyMedium,
            color = SwiftOrange,
        )
        is SegmentDto.Mention -> Text(
            "@npub1${seg.pubkey.take(6)}…",
            style = MaterialTheme.typography.bodyMedium,
            color = Indigo,
            fontWeight = FontWeight.Bold,
        )
        is SegmentDto.Emoji -> {
            val src = seg.url
            if (src != null) {
                AsyncImage(
                    model = src,
                    contentDescription = ":${seg.shortcode}:",
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.size(20.dp),
                )
            } else {
                Text(":${seg.shortcode}:", style = MaterialTheme.typography.bodyMedium)
            }
        }
        else -> Unit
    }
}

/** Inline-text representation for AnnotatedString concatenation (no images). */
private fun androidx.compose.ui.text.AnnotatedString.Builder.appendInline(
    seg: SegmentDto,
    emphasis: Boolean,
    strong: Boolean,
) {
    val baseStyle = SpanStyle(
        fontStyle = if (emphasis) FontStyle.Italic else FontStyle.Normal,
        fontWeight = if (strong) FontWeight.Bold else FontWeight.Normal,
    )
    when (seg) {
        is SegmentDto.Text -> withStyle(baseStyle) { append(seg.text) }
        is SegmentDto.Hashtag -> withStyle(
            baseStyle.copy(color = SwiftAccent, fontWeight = FontWeight.Bold),
        ) { append("#${seg.tag}") }
        is SegmentDto.Url -> withStyle(
            baseStyle.copy(color = SwiftBlue),
        ) { append(seg.url) }
        is SegmentDto.Emoji -> withStyle(baseStyle) { append(":${seg.shortcode}:") }
        is SegmentDto.Invoice -> withStyle(
            baseStyle.copy(color = SwiftOrange),
        ) { append("⚡ ${seg.value.take(12)}…") }
        is SegmentDto.Mention -> withStyle(
            baseStyle.copy(color = Indigo, fontWeight = FontWeight.Bold),
        ) { append("@npub1${seg.pubkey.take(6)}…") }
        else -> Unit // non-inline: dropped by the grouper before we get here.
    }
}

// MARK: - Block segments

@Composable
private fun BlockSegment(
    seg: SegmentDto,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    when (seg) {
        is SegmentDto.Media -> MediaBlock(seg.mediaKind, seg.urls)
        is SegmentDto.EventRef -> EmbedCard(
            uri = seg.uri,
            refId = seg.id,
            entry = embeds[seg.uri],
            embeds = embeds,
            ctx = ctx,
        )
        is SegmentDto.MarkdownBlock -> MarkdownNodeView(
            node = seg.node,
            embeds = embeds,
            ctx = ctx,
        )
        is SegmentDto.Unknown -> Text(
            "[unknown segment: ${seg.type}]",
            style = MaterialTheme.typography.labelSmall,
            color = SwiftRed,
        )
        else -> Unit
    }
}

@Composable
private fun MediaBlock(kind: String, urls: List<String>) {
    val media = LocalNmpMediaRenderer.current
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (urlString in urls) {
            if (urlString.isBlank()) {
                MediaErrorRow(urlString)
            } else when (kind) {
                "Image" -> media.imageView(urlString)
                "Video" -> media.videoView(urlString)
                else -> MediaFallbackLabel(kind, urlString)
            }
        }
    }
}

@Composable
private fun MediaFallbackLabel(kind: String, url: String) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            "[$kind]",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
            color = SwiftPurple,
        )
        Text(
            url,
            style = MaterialTheme.typography.labelSmall,
            color = SwiftPurple,
            maxLines = 1,
        )
    }
}

@Composable
private fun MediaErrorRow(url: String) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Icons.Filled.Warning,
            contentDescription = null,
            tint = SwiftRed,
            modifier = Modifier.size(14.dp),
        )
        Text(
            url,
            style = MaterialTheme.typography.labelSmall.copy(
                fontFamily = FontFamily.Monospace,
            ),
            color = SwiftRed,
            maxLines = 1,
        )
    }
}

/** Top-level dispatcher for a scenario's primary rendered tree. */
@Composable
fun ScenarioRenderer(
    rendered: ContentTreeDto,
    embeds: Map<String, EmbedEntry>,
    modifier: Modifier = Modifier,
) {
    SegmentDtoView(
        tree = rendered,
        embeds = embeds,
        ctx = RenderContext(),
        modifier = modifier,
    )
}

