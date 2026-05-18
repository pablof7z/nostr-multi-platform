package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.ContentTreeDto
import org.nmp.gallery.model.EmbedEntry
import org.nmp.gallery.model.MarkdownInlineDto
import org.nmp.gallery.model.MarkdownNodeDto
import org.nmp.gallery.model.RenderContext
import org.nmp.gallery.model.SegmentDto

/**
 * Compose port of Swift `MarkdownNodeView` + `InlineFlow`. Renders one
 * [MarkdownNodeDto] (CommonMark-core only, PD-012: no tables, no
 * strikethrough). Inline runs reuse the [SegmentDto] shape, so `nostr:`
 * mentions inside an article body resolve through the same embed store.
 */
@Composable
fun MarkdownNodeView(
    node: MarkdownNodeDto,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    when (node) {
        is MarkdownNodeDto.Heading -> Heading(node.level, node.inlines, embeds, ctx)
        is MarkdownNodeDto.Paragraph -> InlineFlow(node.inlines, embeds, ctx)
        is MarkdownNodeDto.BlockQuote -> BlockQuote(node.blocks, embeds, ctx)
        is MarkdownNodeDto.CodeBlock -> CodeBlock(node.info, node.body)
        is MarkdownNodeDto.ListNode -> ListBlock(node.orderedStart, node.items, embeds, ctx)
        MarkdownNodeDto.Rule -> HorizontalDivider(
            modifier = Modifier.padding(vertical = 2.dp),
        )
        is MarkdownNodeDto.Unknown -> Text(
            "[md: ${node.type}]",
            style = MaterialTheme.typography.labelSmall,
            color = SwiftRed,
        )
    }
}

@Composable
private fun Heading(
    level: Int,
    inlines: List<MarkdownInlineDto>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    // Mirror the iOS heading scale (title / title2 / title3 / headline).
    val baseStyle = when (level) {
        1 -> MaterialTheme.typography.headlineMedium
        2 -> MaterialTheme.typography.headlineSmall
        3 -> MaterialTheme.typography.titleLarge
        else -> MaterialTheme.typography.titleMedium
    }
    val style = baseStyle.copy(fontWeight = FontWeight.Bold)
    // Propagate the heading text style down through SegmentDtoView's
    // inline-run renderer via LocalTextStyle (same composition-local
    // pattern as LocalEmphasis / LocalStrong).
    CompositionLocalProvider(
        LocalTextStyle provides style,
        LocalStrong provides true,
    ) {
        InlineFlow(inlines, embeds, ctx)
    }
}

@Composable
private fun BlockQuote(
    blocks: List<MarkdownNodeDto>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    // IntrinsicSize.Min lets the Row measure its tallest child first so the
    // fillMaxHeight bar stretches to match the content column. Without it
    // the bar would lay out at 0dp height (its only inherent size hint).
    Row(modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .fillMaxHeight()
                .background(MaterialTheme.colorScheme.outline),
        )
        Spacer(Modifier.width(10.dp))
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            for (b in blocks) MarkdownNodeView(b, embeds, ctx)
        }
    }
}

@Composable
private fun CodeBlock(info: String?, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        info?.let { i ->
            Text(
                i,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(
            body,
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
            ),
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(6.dp))
                .background(Color.Gray.copy(alpha = 0.12f))
                .padding(8.dp),
        )
    }
}

@Composable
private fun ListBlock(
    orderedStart: Long?,
    items: List<List<MarkdownNodeDto>>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        items.forEachIndexed { idx, item ->
            Row(
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.Top,
            ) {
                Text(
                    marker(orderedStart, idx),
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                )
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    for (b in item) MarkdownNodeView(b, embeds, ctx)
                }
            }
        }
    }
}

private fun marker(start: Long?, idx: Int): String =
    if (start != null) "${start + idx}." else "•"

/**
 * Flattens markdown inline runs to a wrapping text/segment row. Emphasis /
 * strong / code map to text styling; `Inline(Segment)` delegates to the
 * shared [SegmentDtoView] so mentions + event refs inside article bodies
 * resolve identically to plaintext.
 */
@Composable
fun InlineFlow(
    inlines: List<MarkdownInlineDto>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    // Group consecutive inline segments through the SegmentDto inline path
    // for proper word-wrap concatenation; non-segment inlines (link/image)
    // break the run and render as separate widgets. The ambient text style
    // (set by Heading via LocalTextStyle) flows through SegmentDtoView's
    // InlineRun, so headings + body + link tints all render correctly.
    val segmentRun = mutableListOf<SegmentDto>()
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        for (inline in inlines) {
            when (inline) {
                is MarkdownInlineDto.Inline -> segmentRun.add(inline.segment)
                else -> {
                    flushRun(segmentRun, embeds, ctx)
                    segmentRun.clear()
                    when (inline) {
                        is MarkdownInlineDto.Emphasis -> CompositionLocalProvider(
                            LocalEmphasis provides true,
                        ) { InlineFlow(inline.children, embeds, ctx) }
                        is MarkdownInlineDto.Strong -> CompositionLocalProvider(
                            LocalStrong provides true,
                        ) { InlineFlow(inline.children, embeds, ctx) }
                        is MarkdownInlineDto.Code -> InlineCode(inline.text)
                        is MarkdownInlineDto.Link -> InlineLink(inline.label, embeds, ctx)
                        is MarkdownInlineDto.Image -> InlineImage(inline.alt, inline.src)
                        MarkdownInlineDto.SoftBreak,
                        MarkdownInlineDto.HardBreak,
                        -> Unit
                        is MarkdownInlineDto.Unknown -> Text(
                            "[inline: ${inline.type}]",
                            style = MaterialTheme.typography.labelSmall,
                            color = SwiftRed,
                        )
                        is MarkdownInlineDto.Inline -> Unit // handled above
                    }
                }
            }
        }
        if (segmentRun.isNotEmpty()) flushRun(segmentRun, embeds, ctx)
    }
}

@Composable
private fun flushRun(
    run: List<SegmentDto>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    if (run.isEmpty()) return
    SegmentDtoView(
        tree = ContentTreeDto(mode = "Markdown", segments = run.toList()),
        embeds = embeds,
        ctx = ctx,
    )
}

@Composable
private fun InlineCode(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium.copy(
            fontFamily = FontFamily.Monospace,
        ),
        modifier = Modifier
            .clip(RoundedCornerShape(4.dp))
            .background(Color.Gray.copy(alpha = 0.12f))
            .padding(horizontal = 4.dp),
    )
}

@Composable
private fun InlineLink(
    label: List<MarkdownInlineDto>,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("🔗", style = MaterialTheme.typography.labelSmall, color = SwiftBlue)
        // Recurse inline flow; link is rendered as blue-tinted child run.
        InlineFlow(label, embeds, ctx)
    }
}

@Composable
private fun InlineImage(alt: String, src: String?) {
    Text(
        "🖼 $alt [${src ?: "no src"}]",
        style = MaterialTheme.typography.labelSmall,
        color = SwiftPurple,
    )
}
