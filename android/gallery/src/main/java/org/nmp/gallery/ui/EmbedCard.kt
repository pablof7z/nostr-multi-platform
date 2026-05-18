package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.ArticleHeaderDto
import org.nmp.gallery.model.EmbedEntry
import org.nmp.gallery.model.ListDto
import org.nmp.gallery.model.ListRowDto
import org.nmp.gallery.model.RenderContext
import org.nmp.gallery.model.visitedKey

/**
 * Compose port of Swift `EmbedCard`. Embedded event card (quoted note /
 * nevent / naddr). Applies the [RenderContext] PD-015 depth + cycle guard
 * and degrades gracefully for dangling / unsupported targets (D1 — never
 * blank, never a spinner).
 */
@Composable
fun EmbedCard(
    uri: String,
    refId: String,
    entry: EmbedEntry?,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    if (entry == null) {
        StubBox("Quoted event unavailable", refId, glyph = "?")
        return
    }

    // Bundle-time, context-independent collapse facts.
    if (entry.collapsed) {
        when (entry.collapseReason) {
            "dangling" -> StubBox("Quoted event unavailable", refId, glyph = "⌀")
            "unsupported" -> StubBox(
                "Unsupported event (kind ${entry.resolvedKind})",
                refId,
                glyph = "▢",
            )
            else -> StubBox("Embed collapsed", refId, glyph = "▷")
        }
        return
    }

    // Render-time PD-015 depth + cycle guard.
    val ev = entry.event
    if (ev != null) {
        val key = visitedKey(ev)
        val (collapse, reason) = ctx.shouldCollapse(key)
        if (collapse) {
            val label = if (reason == "cycle") {
                "Already shown (cycle broken)"
            } else {
                "Quoted event (tap to open)"
            }
            val glyph = if (reason == "cycle") "↻" else "›"
            StubBox(label, refId, glyph = glyph)
            return
        }
    }

    CardBody(entry, embeds, ctx)
}

@Composable
private fun CardBody(
    entry: EmbedEntry,
    embeds: Map<String, EmbedEntry>,
    ctx: RenderContext,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.06f))
            .border(
                width = 1.dp,
                color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f),
                shape = RoundedCornerShape(8.dp),
            )
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        entry.article?.let { ArticlePreview(it) }
        entry.list?.let { ListCard(it) }

        entry.event?.let { ev ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Identicon(ev.pubkey, modifier = Modifier.size(18.dp))
                Text(
                    "kind ${ev.kind} · @npub1${ev.pubkey.take(6)}…",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        val body = entry.rendered
        if (body != null && entry.article == null && entry.list == null) {
            val childCtx = entry.event?.let { ctx.descend(visitedKey(it)) } ?: ctx
            SegmentDtoView(tree = body, embeds = embeds, ctx = childCtx)
        }
    }
}

@Composable
private fun StubBox(title: String, detail: String, glyph: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(Color.Gray.copy(alpha = 0.10f))
            .padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            glyph,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Column {
            Text(
                title,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
            Text(
                detail.take(28) + "…",
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
        }
    }
}

/** Medium-like article preview card (naddr → kind:30023). */
@Composable
fun ArticlePreview(header: ArticleHeaderDto) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(SwiftTeal.copy(alpha = 0.10f))
            .padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            "[Article]",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
            color = SwiftTeal,
        )
        Text(
            header.title ?: "(untitled)",
            style = MaterialTheme.typography.titleSmall,
        )
        header.summary?.let { s ->
            Text(
                s,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Identicon(header.author, modifier = Modifier.size(16.dp))
            Text(
                "@npub1${header.author.take(6)}…",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            Text(
                "Read article ›",
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = SwiftTeal,
            )
        }
    }
}

/** NIP-51 inline titled list card (follow set / bookmarks / relay list). */
@Composable
fun ListCard(list: ListDto) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(SwiftGreen.copy(alpha = 0.10f))
            .padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            "▤ ${list.title ?: "List"}",
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Bold,
            color = SwiftGreen,
        )
        if (list.rows.isEmpty()) {
            Text(
                "(no members)",
                style = MaterialTheme.typography.labelSmall.copy(
                    fontStyle = FontStyle.Italic,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
        }
        for (row in list.rows) ListRow(row)
    }
}

@Composable
private fun ListRow(row: ListRowDto) {
    when (row) {
        is ListRowDto.Profile -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Identicon(row.pubkey, modifier = Modifier.size(16.dp))
            Text(
                row.name?.let { "@$it" } ?: "@npub1${row.pubkey.take(6)}…",
                style = MaterialTheme.typography.labelMedium,
            )
        }
        is ListRowDto.Event -> Text(
            "“ note ${row.id.take(10)}…",
            style = MaterialTheme.typography.labelSmall,
        )
        is ListRowDto.Address -> Text(
            "▤ ${row.coord}",
            style = MaterialTheme.typography.labelSmall,
            maxLines = 1,
        )
        is ListRowDto.Hashtag -> Text(
            "# ${row.tag}",
            style = MaterialTheme.typography.labelSmall,
        )
        is ListRowDto.Relay -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text("📡", style = MaterialTheme.typography.labelSmall)
            Text(
                row.url,
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
            )
            if (row.read) TagBadge("R")
            if (row.write) TagBadge("W")
        }
        is ListRowDto.Unknown -> Text(
            "[${row.type}]",
            style = MaterialTheme.typography.labelSmall,
            color = SwiftRed,
        )
    }
}

@Composable
private fun TagBadge(label: String) {
    Text(
        label,
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        modifier = Modifier
            .clip(RoundedCornerShape(percent = 50))
            .background(SwiftGreen.copy(alpha = 0.25f))
            .padding(horizontal = 4.dp),
    )
}
