// Requires: compose-ui, compose-foundation, compose-material3. Kotlin 1.9+.
//
// Compose typed renderer for kind:9802 highlights (NIP-84). The model is
// hydrated from Rust's HighlightProjection; this file only formats raw fields.
// Install alongside `compose/content-kind-registry`, then register
// `NostrHighlightCardRenderer` with `registry.setHighlight(...)`.

package org.nmp.gallery.registry

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

public data class NostrHighlightCardModel(
    val id: String,
    val authorPubkey: String? = null,
    val authorDisplayName: String? = null,
    val highlightedText: String,
    val context: String? = null,
    val sourceEventId: String? = null,
    val sourceEventAddr: String? = null,
    val sourceUrl: String? = null,
)

public val NostrHighlightCardRenderer: KindRenderer = KindRenderer { projection, _ ->
    val highlight = projection.highlight ?: return@KindRenderer
    NostrHighlightCard(model = highlight.toNostrHighlightCardModel())
}

public fun HighlightProjection.toNostrHighlightCardModel(): NostrHighlightCardModel =
    NostrHighlightCardModel(
        id = id,
        authorPubkey = authorPubkey,
        highlightedText = highlightedText,
        context = context,
        sourceEventId = sourceEventId,
        sourceEventAddr = sourceEventAddr,
        sourceUrl = sourceUrl,
    )

@Composable
public fun NostrHighlightCard(
    model: NostrHighlightCardModel,
    modifier: Modifier = Modifier,
    onTap: (() -> Unit)? = null,
) {
    val renderer = LocalNostrContentRenderer.current
    val tap = onTap
    val cardModifier = if (tap != null) modifier.clickable { tap() } else modifier
    Column(
        verticalArrangement = Arrangement.spacedBy(10.dp),
        modifier = cardModifier
            .fillMaxWidth()
            .semantics { contentDescription = "Nostr highlight" },
    ) {
        PullQuote(model = model, renderer = renderer)
        HighlightSourceFooter(model = model, renderer = renderer)
        HighlightByline(model = model, renderer = renderer)
    }
}

@Composable
private fun PullQuote(model: NostrHighlightCardModel, renderer: NostrContentRenderer) {
    val accent = Color(0xFFE7B416)
    Row(
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .clip(RoundedCornerShape(6.dp))
            .background(accent.copy(alpha = 0.09f))
            .padding(8.dp),
    ) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .fillMaxHeight()
                .clip(RoundedCornerShape(2.dp))
                .background(accent.copy(alpha = 0.82f)),
        )
        Column(
            verticalArrangement = Arrangement.spacedBy(6.dp),
            modifier = Modifier.weight(1f),
        ) {
            Text(
                text = "“${model.highlightedText}”",
                style = MaterialTheme.typography.bodyLarge.copy(fontStyle = FontStyle.Italic),
                color = renderer.textColor,
            )
            model.context.clean()?.let { context ->
                Text(
                    text = context,
                    style = MaterialTheme.typography.bodySmall,
                    color = renderer.secondaryTextColor,
                )
            }
        }
    }
}

@Composable
private fun HighlightSourceFooter(model: NostrHighlightCardModel, renderer: NostrContentRenderer) {
    val source = highlightSource(model, renderer.callbacks) ?: return
    val tap = source.onTap
    val rowModifier = if (tap != null) Modifier.clickable { tap() } else Modifier
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = rowModifier.fillMaxWidth(),
    ) {
        Text(
            text = source.label,
            style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
            color = renderer.secondaryTextColor,
        )
        Text(
            text = source.displayValue,
            style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontFamily.Monospace),
            color = if (source.isLink) renderer.linkColor else renderer.secondaryTextColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun HighlightByline(model: NostrHighlightCardModel, renderer: NostrContentRenderer) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "highlighted by ${authorLabel(model)}",
            style = MaterialTheme.typography.bodySmall,
            fontWeight = FontWeight.Medium,
            color = renderer.secondaryTextColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Text(
            text = "kind:9802",
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = renderer.secondaryTextColor.copy(alpha = 0.7f),
        )
    }
}

private data class HighlightSource(
    val label: String,
    val displayValue: String,
    val isLink: Boolean = false,
    val onTap: (() -> Unit)? = null,
)

private fun highlightSource(
    model: NostrHighlightCardModel,
    callbacks: NostrContentCallbacks,
): HighlightSource? {
    model.sourceUrl.clean()?.let { url ->
        return HighlightSource(
            label = "link",
            displayValue = url,
            isLink = true,
            onTap = { callbacks.onLinkTap(url) },
        )
    }
    model.sourceEventId.clean()?.let { eventId ->
        return HighlightSource(
            label = "note",
            displayValue = shortHead(eventId),
            onTap = { callbacks.onEventRefTap(eventId) },
        )
    }
    model.sourceEventAddr.clean()?.let { addr ->
        return HighlightSource(
            label = "addr",
            displayValue = addr,
            onTap = { callbacks.onEventRefTap(addr) },
        )
    }
    return null
}

private fun authorLabel(model: NostrHighlightCardModel): String {
    model.authorDisplayName.clean()?.let { return it }
    model.authorPubkey.clean()?.let { return shortHex(it) }
    return "highlight"
}

private fun String?.clean(): String? = this?.trim()?.takeIf { it.isNotEmpty() }

private fun shortHex(value: String): String =
    if (value.length <= 16) value else "${value.take(8)}…${value.takeLast(8)}"

private fun shortHead(value: String): String =
    if (value.length <= 10) value else "${value.take(8)}…"
