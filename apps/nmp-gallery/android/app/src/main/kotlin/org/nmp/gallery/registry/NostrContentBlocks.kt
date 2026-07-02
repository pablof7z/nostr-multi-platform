package org.nmp.gallery.registry

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.HelpOutline
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

@Composable
internal fun MediaGroupBlock(urls: List<String>, kind: MediaKind) {
    when (kind) {
        MediaKind.Image -> {
            val nonEmpty = urls.filter { it.isNotEmpty() }
            if (nonEmpty.isNotEmpty()) {
                NostrMediaGrid(imageUrls = nonEmpty)
            }
        }
        MediaKind.Video, MediaKind.Audio -> {
            val first = urls.firstOrNull()
            if (first != null) {
                MediaRow(url = first, isAudio = kind == MediaKind.Audio)
            }
        }
    }
}

@Composable
private fun MediaRow(url: String, isAudio: Boolean) {
    val renderer = LocalNostrContentRenderer.current
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(renderer.codeBackgroundColor)
            .clickable { renderer.callbacks.onLinkTap(url) }
            .padding(12.dp),
    ) {
        Icon(
            imageVector = if (isAudio) Icons.Filled.VolumeUp else Icons.Filled.PlayArrow,
            contentDescription = null,
            tint = renderer.linkColor,
        )
        Text(
            text = url.substringAfterLast('/').ifEmpty { url },
            color = renderer.secondaryTextColor,
            fontFamily = FontFamily.Monospace,
            maxLines = 1,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
internal fun EventRefBlock(
    uri: WireNostrUri,
) {
    EmbeddedEvent(
        uri = uri.uri,
        primaryId = uri.primaryId,
    )
}

@Composable
internal fun CodeBlockBlock(info: String?, body: String) {
    val renderer = LocalNostrContentRenderer.current
    Column(
        verticalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(6.dp))
            .background(renderer.codeBackgroundColor)
            .padding(10.dp),
    ) {
        if (!info.isNullOrEmpty()) {
            Text(
                text = info,
                color = renderer.secondaryTextColor,
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.labelSmall,
            )
        }
        Text(
            text = body,
            color = renderer.textColor,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
internal fun BlockQuoteBlock(
    children: List<UInt>,
    tree: ContentTreeWire,
    textStyle: TextStyle,
    mentionLabel: (WireNostrUri) -> String,
) {
    val renderer = LocalNostrContentRenderer.current
    val annotated = buildAnnotatedString {
        for (child in children) {
            appendInline(child, tree, renderer, mentionLabel)
        }
    }
    Row(
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(vertical = 4.dp),
    ) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .fillMaxHeight()
                .background(renderer.quoteBorderColor),
        )
        Text(
            text = annotated,
            color = renderer.secondaryTextColor,
            style = textStyle.copy(fontStyle = FontStyle.Italic),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
internal fun ListBlock(
    orderedStart: ULong?,
    items: List<List<UInt>>,
    tree: ContentTreeWire,
    textStyle: TextStyle,
    mentionLabel: (WireNostrUri) -> String,
) {
    val renderer = LocalNostrContentRenderer.current
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.forEachIndexed { offset, children ->
            val annotated = buildAnnotatedString {
                for (child in children) {
                    appendInline(child, tree, renderer, mentionLabel)
                }
            }
            Row(
                verticalAlignment = Alignment.Top,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = marker(orderedStart, offset),
                    color = renderer.secondaryTextColor,
                    style = textStyle,
                )
                Text(
                    text = annotated,
                    color = renderer.textColor,
                    style = textStyle,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

private fun marker(orderedStart: ULong?, offset: Int): String {
    if (orderedStart != null) {
        return "${orderedStart + offset.toULong()}."
    }
    return "•"
}

@Composable
internal fun RuleBlock() {
    val renderer = LocalNostrContentRenderer.current
    Spacer(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .height(1.dp)
            .background(renderer.quoteBorderColor),
    )
}

@Composable
internal fun ImageBlock(alt: String, @Suppress("UNUSED_PARAMETER") title: String?, src: String?) {
    val renderer = LocalNostrContentRenderer.current
    if (!src.isNullOrEmpty()) {
        NostrMediaGrid(imageUrls = listOf(src))
        return
    }
    Text(
        text = if (alt.isEmpty()) "[image]" else "[$alt]",
        color = renderer.placeholderColor,
        style = MaterialTheme.typography.bodySmall,
    )
}

@Composable
internal fun PlaceholderChip(reason: PlaceholderReason) {
    val renderer = LocalNostrContentRenderer.current
    val (label, icon) = when (reason) {
        PlaceholderReason.DepthLimit -> "Nested content collapsed" to Icons.Filled.ExpandMore
        PlaceholderReason.UnresolvedUri -> "Unresolved reference" to Icons.Filled.HelpOutline
    }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        modifier = Modifier
            .clip(RoundedCornerShape(percent = 50))
            .background(renderer.codeBackgroundColor)
            .padding(horizontal = 8.dp, vertical = 4.dp),
    ) {
        Icon(imageVector = icon, contentDescription = null, tint = renderer.placeholderColor)
        Text(
            text = label,
            color = renderer.placeholderColor,
            style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.SemiBold),
        )
    }
}
