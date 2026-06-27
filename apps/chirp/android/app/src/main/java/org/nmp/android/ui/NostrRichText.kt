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
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ContentTreeWire
import org.nmp.android.model.ContentWireNode
import org.nmp.android.model.TimelineItem
import org.nmp.android.model.WireNostrUri
import org.nmp.android.ui.embed.EmbeddedEvent

/**
 * Renders the Rust-produced `nmp_content::ContentTreeWire` arena. Android does
 * not scan content for Nostr entities; protocol tokenization stays in Rust.
 */
@Composable
fun NostrRichText(
    content: String,
    contentTree: ContentTreeWire?,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
    embedDepth: Int,
    modifier: Modifier = Modifier,
) {
    if (contentTree == null || contentTree.roots.isEmpty()) {
        Text(content, modifier = modifier, style = MaterialTheme.typography.bodyMedium)
        return
    }
    // Demand-driven kind:0 fetch for every NIP-21 profile mention referenced
    // by this content tree. Inline mentions render as styled spans inside a
    // single `Text` (not individual composables), so per-mention
    // `DisposableEffect` isn't reachable from `appendInline`; instead the
    // tree is walked once here to collect the mention pubkey set and a
    // single tracker claims/releases the set as a batch. The kernel's
    // `resolve_ref` is idempotent on `(pubkey, consumer_id)` so duplicate
    // entries within the same tree are coalesced kernel-side.
    val mentionPubkeys = remember(contentTree) { collectMentionPubkeys(contentTree) }
    ClaimMentionProfiles(mentionPubkeys)
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        contentTree.roots.forEach { index ->
            ContentNode(index, contentTree, items, cards, embedDepth)
        }
    }
}

/**
 * Walk every node in the content tree's arena and collect the set of
 * 64-hex pubkeys appearing as profile mentions. Returned as a stable-sorted
 * list so `remember(contentTree)` produces a stable key for
 * [ClaimMentionProfiles].
 */
private fun collectMentionPubkeys(tree: ContentTreeWire): List<String> {
    val set = sortedSetOf<String>()
    tree.nodes.forEach { node ->
        if (node is ContentWireNode.MentionNode) {
            val id = node.uri.primaryId
            if (id.length == 64 && id.all { ch ->
                    ch.isDigit() || ch in 'a'..'f' || ch in 'A'..'F'
                }) {
                set.add(id)
            }
        }
    }
    return set.toList()
}

/**
 * One [DisposableEffect] per mention pubkey. Hoisted out of [NostrRichText]
 * so the surrounding `Column` stays single-purpose. Each `RememberProfileClaim`
 * is its own composable subtree, so the claim/release lifecycle is independent
 * per pubkey — if the tree shrinks (an edit drops one mention) only the
 * dropped pubkey is released; the others' DisposableEffects survive.
 */
@Composable
private fun ClaimMentionProfiles(pubkeys: List<String>) {
    pubkeys.forEach { pubkey ->
        RememberProfileClaim(pubkey, "mention-$pubkey")
    }
}

@Composable
private fun ContentNode(
    index: Int,
    tree: ContentTreeWire,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
    embedDepth: Int,
) {
    when (val node = tree.node(index)) {
        is ContentWireNode.MediaNode -> MediaBlock(node.urls, node.mediaKind)
        is ContentWireNode.EventRefNode -> EventRefBlock(node.uri, items, cards, embedDepth)
        is ContentWireNode.ImageNode -> ImageBlock(node)
        is ContentWireNode.InvoiceNode -> InvoiceBlock(node)
        is ContentWireNode.CodeBlockNode -> CodeBlock(node.body, node.info)
        is ContentWireNode.ListNode -> ListBlock(node, tree)
        is ContentWireNode.BlockQuoteNode -> BlockQuote(node.children, tree)
        is ContentWireNode.RuleNode -> SurfaceLine()
        is ContentWireNode.PlaceholderNode -> Placeholder(node.reason)
        null -> Placeholder("depth_limit")
        else -> Text(
            inlineText(listOf(index), tree),
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun MediaBlock(urls: List<String>, mediaKind: String) {
    if (urls.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        urls.forEach { url ->
            when (mediaKind) {
                "Image" -> RemoteImage(url)
                "Video" -> RemoteVideo(url)
                "Audio" -> RemoteAudio(url)
                else -> MediaLink(url, mediaKind)
            }
        }
    }
}

@Composable
private fun EventRefBlock(
    uri: WireNostrUri,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
    embedDepth: Int,
) {
    val eventId = uri.primaryId
    // Beyond the recursion guard, never claim or recurse — show a static
    // placeholder so a self-referential embed chain terminates (mirrors the
    // iOS `EmbedChromeContainer` depth cap).
    if (embedDepth >= MaxEmbedDepth) {
        PendingEventRef(eventId.ifEmpty { uri.uri })
        return
    }
    // In-feed fast path: the referenced event is already in the timeline maps,
    // so render the rich NoteRow (avatar + content + action counts) directly —
    // no embed claim is needed (the feed already carries it).
    if (items.containsKey(eventId) || cards.containsKey(eventId)) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
            tonalElevation = 1.dp,
        ) {
            NoteRow(
                eventId = eventId,
                items = items,
                cards = cards,
                embedDepth = embedDepth + 1,
                embedded = true,
            )
        }
        return
    }
    // Out-of-feed ref (#984): claim the URI so the kernel resolves it and ships
    // the typed projection in the `NEMB` sidecar; `EmbeddedEvent` reads the
    // resolved envelope from `LocalRefEventEnvelopes` and dispatches it through
    // `NostrKindRegistry`. Until resolution lands it shows a loading state, not
    // a permanent placeholder.
    EmbeddedEvent(
        uri = uri.uri,
        primaryId = eventId,
        consumerId = "embed-ref-${eventId.ifEmpty { uri.uri }}",
    )
}

@Composable
private fun MediaLink(url: String, mediaKind: String) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 1.dp,
    ) {
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(mediaKind.lowercase(), style = MaterialTheme.typography.labelMedium)
            Text(url, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace)
        }
    }
}

@Composable
private fun PendingEventRef(value: String) {
    Text(
        "Event pending ${shortEntity(value)}",
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f), RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.06f), RoundedCornerShape(8.dp))
            .padding(10.dp),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
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
private fun ImageBlock(node: ContentWireNode.ImageNode) {
    val src = node.src
    if (src.isNullOrEmpty()) {
        Placeholder("unresolved_uri")
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        RemoteImage(src)
        val caption = node.title ?: node.alt.takeIf { it.isNotEmpty() }
        if (!caption.isNullOrEmpty()) {
            Text(
                caption,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun InvoiceBlock(node: ContentWireNode.InvoiceNode) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 1.dp,
    ) {
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(node.invoiceKind, style = MaterialTheme.typography.labelMedium)
            Text(node.payload, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace)
        }
    }
}

@Composable
private fun Placeholder(reason: String) {
    Text(
        "Unsupported content: $reason",
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

private const val MaxEmbedDepth = 2
