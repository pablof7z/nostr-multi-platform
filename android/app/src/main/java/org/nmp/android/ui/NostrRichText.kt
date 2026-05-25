package org.nmp.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ContentTreeWire
import org.nmp.android.model.ContentWireNode
import org.nmp.android.model.WireNostrUri

/**
 * Renders the Rust-produced `nmp_content::ContentTreeWire` arena. Android does
 * not scan content for Nostr entities; protocol tokenization stays in Rust.
 */
@Composable
fun NostrRichText(
    content: String,
    contentTree: ContentTreeWire?,
    modifier: Modifier = Modifier,
) {
    if (contentTree == null || contentTree.roots.isEmpty()) {
        Text(content, modifier = modifier, style = MaterialTheme.typography.bodyMedium)
        return
    }
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        contentTree.roots.forEach { index ->
            ContentNode(index, contentTree)
        }
    }
}

@Composable
private fun ContentNode(index: Int, tree: ContentTreeWire) {
    when (val node = tree.node(index)) {
        is ContentWireNode.MediaNode -> MediaBlock(node.urls, node.mediaKind)
        is ContentWireNode.EventRefNode -> EventRefBlock(node.uri)
        is ContentWireNode.ImageNode -> MediaBlock(listOfNotNull(node.src), "Image")
        is ContentWireNode.CodeBlockNode -> CodeBlock(node.body, node.info)
        is ContentWireNode.ListNode -> ListBlock(node, tree)
        is ContentWireNode.BlockQuoteNode -> BlockQuote(node.children, tree)
        is ContentWireNode.RuleNode -> SurfaceLine()
        is ContentWireNode.PlaceholderNode -> Placeholder()
        null -> Placeholder()
        else -> Text(
            inlineText(listOf(index), tree),
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun MediaBlock(urls: List<String>, mediaKind: String) {
    if (urls.isEmpty()) return
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 1.dp,
    ) {
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(mediaKind.lowercase(), style = MaterialTheme.typography.labelMedium)
            urls.take(3).forEach {
                Text(it, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace)
            }
        }
    }
}

@Composable
private fun EventRefBlock(uri: WireNostrUri) {
    val label = shortEntity(uri.primaryId).ifEmpty { uri.uri }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f), RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text("Referenced event", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold)
        Text(label, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun CodeBlock(body: String, info: String?) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color.Gray.copy(alpha = 0.08f), RoundedCornerShape(8.dp))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (!info.isNullOrEmpty()) {
            Text(info, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Text(body, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun ListBlock(node: ContentWireNode.ListNode, tree: ContentTreeWire) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        node.items.forEachIndexed { index, children ->
            val marker = node.orderedStart?.let { "${it + index}." } ?: "•"
            Text(buildAnnotatedString {
                append("$marker ")
                append(inlineText(children, tree))
            })
        }
    }
}

@Composable
private fun BlockQuote(children: List<Int>, tree: ContentTreeWire) {
    Text(
        inlineText(children, tree),
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.3f), RoundedCornerShape(6.dp))
            .padding(8.dp),
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun SurfaceLine() {
    HorizontalDivider(Modifier.fillMaxWidth().padding(vertical = 6.dp))
}

@Composable
private fun Placeholder() {
    Text(
        "Unsupported content",
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

private fun inlineText(indices: List<Int>, tree: ContentTreeWire): AnnotatedString =
    buildAnnotatedString {
        indices.forEach { appendInline(it, tree) }
    }

private fun AnnotatedString.Builder.appendInline(index: Int, tree: ContentTreeWire) {
    when (val node = tree.node(index)) {
        is ContentWireNode.TextNode -> append(node.text)
        is ContentWireNode.MentionNode -> styled("@${shortEntity(node.uri.primaryId)}", MentionAccent, bold = true)
        is ContentWireNode.EventRefNode -> styled("↩ ${shortEntity(node.uri.primaryId)}", MentionAccent, bold = true)
        is ContentWireNode.HashtagNode -> styled("#${node.tag}", MentionAccent, bold = true)
        is ContentWireNode.UrlNode -> styled(node.url, MentionAccent)
        is ContentWireNode.EmojiNode -> append(":${node.shortcode}:")
        is ContentWireNode.ParagraphNode -> appendChildren(node.children, tree)
        is ContentWireNode.HeadingNode -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
            appendChildren(node.children, tree)
        }
        is ContentWireNode.EmphasisNode -> withStyle(SpanStyle(fontWeight = FontWeight.Medium)) {
            appendChildren(node.children, tree)
        }
        is ContentWireNode.StrongNode -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
            appendChildren(node.children, tree)
        }
        is ContentWireNode.InlineCodeNode -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) {
            append(node.code)
        }
        is ContentWireNode.LinkNode -> styled(inlineText(node.children, tree).text, MentionAccent)
        is ContentWireNode.SoftBreakNode -> append(" ")
        is ContentWireNode.HardBreakNode -> append("\n")
        else -> Unit
    }
}

private fun AnnotatedString.Builder.appendChildren(indices: List<Int>, tree: ContentTreeWire) {
    indices.forEach { appendInline(it, tree) }
}

private fun AnnotatedString.Builder.styled(value: String, color: Color, bold: Boolean = false) {
    withStyle(
        SpanStyle(
            color = color,
            fontWeight = if (bold) FontWeight.Bold else null,
        ),
    ) {
        append(value)
    }
}

private fun ContentTreeWire.node(index: Int): ContentWireNode? =
    nodes.getOrNull(index)

private fun shortEntity(value: String): String {
    if (value.length <= 16) return value
    return "${value.take(8)}…${value.takeLast(8)}"
}

private val MentionAccent = Color(0xFF5856D6)
