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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.ContentTreeWire
import org.nmp.gallery.model.EmbedEntry
import org.nmp.gallery.model.MediaKind
import org.nmp.gallery.model.PlaceholderReason
import org.nmp.gallery.model.RenderContext
import org.nmp.gallery.model.WireInvoice
import org.nmp.gallery.model.WireNode

@Composable
fun WireNodeView(
    tree: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext = RenderContext(),
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        val groups = wireNodeGroups(tree)
        if (groups.isEmpty()) {
            EmptyContent()
        }
        for (group in groups) {
            RenderGroup(group, tree, embeds, ctx)
        }
    }
}

@Composable
private fun RenderGroup(
    group: NodeGroup,
    tree: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    when (group) {
        is NodeGroup.Inline -> InlineRun(group.level, group.children, tree, embeds)
        is NodeGroup.Media -> MediaBlock(group.kind, group.urls)
        is NodeGroup.EventRef -> EmbeddedEvent(
            uri = group.uri,
            entry = embeds[group.uri.uri],
            embeds = embeds,
            ctx = ctx,
        )
        is NodeGroup.CodeBlock -> CodeBlock(group.info, group.body)
        is NodeGroup.BlockQuote -> BlockQuote(tree.withRoots(group.children), embeds, ctx)
        is NodeGroup.ListBlock -> ListBlock(group.orderedStart, group.items, tree, embeds, ctx)
        NodeGroup.Rule -> HorizontalDivider(modifier = Modifier.padding(vertical = 2.dp))
        is NodeGroup.Image -> ImageBlock(group.alt, group.src)
        is NodeGroup.Placeholder -> PlaceholderChip(group.reason)
    }
}

private sealed class NodeGroup {
    data class Inline(val level: InlineLevel, val children: List<UInt>) : NodeGroup()
    data class Media(val kind: MediaKind, val urls: List<String>) : NodeGroup()
    data class EventRef(val uri: org.nmp.gallery.model.WireNostrUri) : NodeGroup()
    data class CodeBlock(val info: String?, val body: String) : NodeGroup()
    data class BlockQuote(val children: List<UInt>) : NodeGroup()
    data class ListBlock(val orderedStart: ULong?, val items: List<List<UInt>>) : NodeGroup()
    object Rule : NodeGroup()
    data class Image(val alt: String, val src: String?) : NodeGroup()
    data class Placeholder(val reason: PlaceholderReason) : NodeGroup()
}

private sealed class InlineLevel {
    object Paragraph : InlineLevel()
    data class Heading(val level: UByte) : InlineLevel()
}

private val NewlineSentinel: UInt = UInt.MAX_VALUE

private fun wireNodeGroups(tree: ContentTreeWire): List<NodeGroup> {
    val groups = mutableListOf<NodeGroup>()
    var pending = mutableListOf<UInt>()
    var pendingLevel: InlineLevel = InlineLevel.Paragraph

    fun flush() {
        if (pending.isNotEmpty()) {
            groups.add(NodeGroup.Inline(pendingLevel, pending.toList()))
            pending = mutableListOf()
            pendingLevel = InlineLevel.Paragraph
        }
    }

    fun appendInline(level: InlineLevel, children: List<UInt>, trailingBreak: Boolean) {
        if (pending.isNotEmpty() && pendingLevel != level) flush()
        pendingLevel = level
        val startCount = pending.size
        for (child in children) {
            when (val childNode = tree.nodeAt(child)) {
                is WireNode.EventRef -> {
                    flush()
                    groups.add(NodeGroup.EventRef(childNode.uri))
                }
                null -> Unit
                else -> pending.add(child)
            }
        }
        if (trailingBreak && pending.size > startCount) {
            pending.add(NewlineSentinel)
        }
    }

    for (root in tree.roots) {
        when (val node = tree.nodeAt(root)) {
            is WireNode.Paragraph -> appendInline(InlineLevel.Paragraph, node.children, true)
            is WireNode.Heading -> {
                flush()
                appendInline(InlineLevel.Heading(node.level), node.children, true)
                flush()
            }
            is WireNode.Media -> {
                flush()
                groups.add(NodeGroup.Media(node.mediaKind, node.urls))
            }
            is WireNode.EventRef -> {
                flush()
                groups.add(NodeGroup.EventRef(node.uri))
            }
            is WireNode.CodeBlock -> {
                flush()
                groups.add(NodeGroup.CodeBlock(node.info, node.body))
            }
            is WireNode.BlockQuote -> {
                flush()
                groups.add(NodeGroup.BlockQuote(node.children))
            }
            is WireNode.ListNode -> {
                flush()
                groups.add(NodeGroup.ListBlock(node.orderedStart, node.items))
            }
            WireNode.Rule -> {
                flush()
                groups.add(NodeGroup.Rule)
            }
            is WireNode.Image -> {
                flush()
                groups.add(NodeGroup.Image(node.alt, node.src))
            }
            is WireNode.Placeholder -> {
                flush()
                groups.add(NodeGroup.Placeholder(node.reason))
            }
            is WireNode.Text,
            is WireNode.Mention,
            is WireNode.Hashtag,
            is WireNode.Url,
            is WireNode.Emoji,
            is WireNode.Invoice,
            is WireNode.Emphasis,
            is WireNode.Strong,
            is WireNode.InlineCode,
            is WireNode.Link,
            WireNode.SoftBreak,
            WireNode.HardBreak -> appendInline(InlineLevel.Paragraph, listOf(root), false)
            null -> Unit
        }
    }
    flush()
    return groups
}

@Composable
private fun EmptyContent() {
    Text(
        "(empty content)",
        style = MaterialTheme.typography.labelSmall.copy(fontStyle = FontStyle.Italic),
        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
    )
}

@Composable
private fun InlineRun(
    level: InlineLevel,
    children: List<UInt>,
    tree: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
) {
    val text = buildAnnotatedString {
        for (child in children) appendInline(child, tree, embeds)
    }.trimTrailingBreak()
    val style = when (level) {
        InlineLevel.Paragraph -> MaterialTheme.typography.bodyMedium
        is InlineLevel.Heading -> when (level.level.toInt()) {
            1 -> MaterialTheme.typography.headlineMedium
            2 -> MaterialTheme.typography.headlineSmall
            3 -> MaterialTheme.typography.titleLarge
            else -> MaterialTheme.typography.titleMedium
        }.copy(fontWeight = FontWeight.Bold)
    }
    Text(text, style = style)
}

private fun AnnotatedString.Builder.appendInline(
    index: UInt,
    tree: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
) {
    if (index == NewlineSentinel) {
        append('\n')
        return
    }
    when (val node = tree.nodeAt(index)) {
        is WireNode.Text -> append(node.text)
        is WireNode.Mention -> {
            val profile = embeds[node.uri.uri]
            val label = profile?.profileName ?: "npub1${node.uri.primaryId.take(6)}..."
            withStyle(SpanStyle(color = Indigo, fontWeight = FontWeight.Bold)) {
                append("@$label")
            }
        }
        is WireNode.EventRef -> withStyle(
            SpanStyle(color = SwiftPurple, fontWeight = FontWeight.Bold),
        ) {
            append("quote ${node.uri.primaryId.take(10)}...")
        }
        is WireNode.Hashtag -> withStyle(
            SpanStyle(color = SwiftAccent, fontWeight = FontWeight.Bold),
        ) {
            append("#${node.tag}")
        }
        is WireNode.Url -> withStyle(SpanStyle(color = SwiftBlue)) {
            append(node.url)
        }
        is WireNode.Emoji -> append(":${node.shortcode}:")
        is WireNode.Invoice -> withStyle(SpanStyle(color = SwiftOrange)) {
            append(node.invoice.label())
        }
        is WireNode.Emphasis -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) {
            for (child in node.children) appendInline(child, tree, embeds)
        }
        is WireNode.Strong -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
            for (child in node.children) appendInline(child, tree, embeds)
        }
        is WireNode.InlineCode -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) {
            append(node.code)
        }
        is WireNode.Link -> {
            val style = SpanStyle(color = SwiftBlue, textDecoration = TextDecoration.Underline)
            withStyle(style) {
                if (node.children.isEmpty()) {
                    append(node.href ?: "")
                } else {
                    for (child in node.children) appendInline(child, tree, embeds)
                }
            }
        }
        is WireNode.Image -> append(if (node.alt.isBlank()) "[image]" else "[${node.alt}]")
        WireNode.SoftBreak -> append(' ')
        WireNode.HardBreak -> append('\n')
        is WireNode.Paragraph -> node.children.forEach { appendInline(it, tree, embeds) }
        is WireNode.Heading -> node.children.forEach { appendInline(it, tree, embeds) }
        is WireNode.BlockQuote -> node.children.forEach { appendInline(it, tree, embeds) }
        is WireNode.Media -> append("[${node.mediaKind.name.lowercase()} media]")
        is WireNode.CodeBlock -> append(node.body)
        is WireNode.ListNode -> append("[list]")
        WireNode.Rule -> append("---")
        is WireNode.Placeholder -> append("[${node.reason.name.lowercase()}]")
        null -> Unit
    }
}

private fun AnnotatedString.trimTrailingBreak(): AnnotatedString {
    val text = toString().trimEnd('\n')
    if (text.length == length) return this
    return subSequence(0, text.length)
}

private fun WireInvoice.label(): String {
    val value = bolt11 ?: bolt12 ?: cashu ?: return "invoice"
    return "invoice ${value.take(12)}..."
}

@Composable
private fun MediaBlock(kind: MediaKind, urls: List<String>) {
    val media = LocalNmpMediaRenderer.current
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        for (url in urls) {
            if (url.isBlank()) {
                MediaErrorRow(url)
            } else when (kind) {
                MediaKind.Image -> media.imageView(url)
                MediaKind.Video -> media.videoView(url)
                MediaKind.Audio -> MediaFallbackLabel("Audio", url)
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
            style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
            color = SwiftRed,
            maxLines = 1,
        )
    }
}

@Composable
private fun CodeBlock(info: String?, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        info?.let {
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(
            body,
            style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(6.dp))
                .background(androidx.compose.ui.graphics.Color.Gray.copy(alpha = 0.12f))
                .padding(8.dp),
        )
    }
}

@Composable
private fun BlockQuote(tree: ContentTreeWire, embeds: Map<String, EmbedEntry>, ctx: RenderContext) {
    Row(modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .fillMaxHeight()
                .background(MaterialTheme.colorScheme.outline),
        )
        Spacer(Modifier.width(10.dp))
        WireNodeView(
            tree = tree,
            embeds = embeds,
            ctx = ctx,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun ListBlock(
    orderedStart: ULong?,
    items: List<List<UInt>>,
    tree: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        items.forEachIndexed { index, item ->
            Row(
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.Top,
            ) {
                Text(
                    marker(orderedStart, index),
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                )
                WireNodeView(
                    tree = tree.withRoots(item),
                    embeds = embeds,
                    ctx = ctx,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

private fun marker(start: ULong?, index: Int): String =
    if (start != null) "${start + index.toULong()}." else "-"

@Composable
private fun ImageBlock(alt: String, src: String?) {
    if (!src.isNullOrBlank()) {
        MediaBlock(MediaKind.Image, listOf(src))
    } else {
        Text(
            if (alt.isBlank()) "[image]" else "[image: $alt]",
            style = MaterialTheme.typography.labelSmall,
            color = SwiftPurple,
        )
    }
}

@Composable
private fun PlaceholderChip(reason: PlaceholderReason) {
    Text(
        "[${reason.name.lowercase()}]",
        style = MaterialTheme.typography.labelSmall.copy(fontStyle = FontStyle.Italic),
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
fun ScenarioRenderer(
    rendered: ContentTreeWire,
    embeds: Map<String, EmbedEntry>,
    modifier: Modifier = Modifier,
) {
    WireNodeView(
        tree = rendered,
        embeds = embeds,
        ctx = RenderContext(),
        modifier = modifier,
    )
}
