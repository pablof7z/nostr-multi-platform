package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.ChirpEventCard
import org.nmp.android.model.ModuleTimelineBlock
import org.nmp.android.model.StandaloneTimelineBlock
import org.nmp.android.model.TimelineBlock
import org.nmp.android.model.TimelineItem

/**
 * Live kind:1 feed straight from the kernel snapshot — Android peer of iOS
 * `TimelineView`. Renders verbatim; no sorting/derivation (D8).
 */
@Composable
fun TimelineScreen(model: KernelModel, modifier: Modifier = Modifier) {
    LaunchedEffect(model) {
        model.openTimeline()
    }
    val s by model.state.collectAsStateWithLifecycle()
    val snapshotCount by model.snapshotCount.collectAsStateWithLifecycle()
    val activeAccount = s.projections
        ?.accounts
        ?.firstOrNull { it.id == s.activeAccount }
    val itemLookup = s.items.associateBy { it.id }
    val cardLookup = s.modularTimeline.cards.associateBy { it.id }
    val blocks = if (s.modularTimeline.blocks.isNotEmpty()) {
        s.modularTimeline.blocks
    } else {
        s.items.map { StandaloneTimelineBlock(it.id) }
    }

    Column(modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text("Chirp", style = MaterialTheme.typography.headlineSmall)
            Text(
                "rev ${s.rev} · ${blocks.size} blocks",
                style = MaterialTheme.typography.labelSmall,
            )
        }
        HorizontalDivider()
        if (blocks.isEmpty()) {
            Placeholder(
                activeAccountLabel = activeAccount?.npubShort ?: s.activeAccount,
                hasAccount = s.activeAccount.isNotEmpty(),
                hasSnapshot = snapshotCount > 0,
                lastErrorToast = s.lastErrorToast,
                onCreateAccount = { model.createLocalAccount() },
            )
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                itemsIndexed(blocks, key = { index, block -> blockKey(index, block) }) { _, block ->
                    TimelineBlockRow(block, itemLookup, cardLookup)
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun Placeholder(
    activeAccountLabel: String,
    hasAccount: Boolean,
    hasSnapshot: Boolean,
    lastErrorToast: String?,
    onCreateAccount: () -> Unit,
) {
    val message = if (hasAccount) {
        "No timeline events yet"
    } else {
        lastErrorToast?.nonEmptyOrNull() ?: if (hasSnapshot) "No active account" else "Starting kernel…"
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            if (!hasSnapshot) {
                CircularProgressIndicator()
                Spacer(Modifier.size(16.dp))
            }
            Text(
                message,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            if (hasAccount) {
                Spacer(Modifier.size(8.dp))
                Text(
                    "Active account: $activeAccountLabel",
                    style = MaterialTheme.typography.bodySmall,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 24.dp),
                )
            } else if (hasSnapshot) {
                Spacer(Modifier.size(16.dp))
                Button(onClick = onCreateAccount) {
                    Text("Create local account")
                }
            }
        }
    }
}

@Composable
private fun TimelineBlockRow(
    block: TimelineBlock,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
) {
    when (block) {
        is StandaloneTimelineBlock -> NoteRow(block.eventId, items, cards)
        is ModuleTimelineBlock -> ModuleBlockRow(block, items, cards)
    }
}

@Composable
private fun ModuleBlockRow(
    block: ModuleTimelineBlock,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
) {
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        block.events.forEachIndexed { index, eventId ->
            NoteRow(eventId, items, cards)
            if (index < block.events.lastIndex) {
                HorizontalDivider(Modifier.padding(start = 56.dp))
            }
        }
        if (block.hasGap) {
            Text(
                "Thread has more context",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 56.dp, bottom = 8.dp),
            )
        }
    }
}

@Composable
private fun NoteRow(
    eventId: String,
    items: Map<String, TimelineItem>,
    cards: Map<String, ChirpEventCard>,
) {
    val item = items[eventId]
    val card = cards[eventId]
    val content = item?.contentPreview?.ifEmpty { item.content } ?: card?.content
    if (content == null) {
        MissingEventRow(eventId)
        return
    }
    val author = item?.authorDisplay?.nonEmptyOrNull()
        ?: card?.authorPubkey?.take(12)?.let { "$it…" }
        ?: "unknown"
    val initials = item?.authorAvatarInitials?.nonEmptyOrNull()
        ?: author.take(2).uppercase()
    val color = item?.authorAvatarColor.orEmpty()
    val subtitle = item?.createdAtDisplay?.nonEmptyOrNull()
        ?: card?.let { "kind ${it.kind}" }
        ?: ""

    Column(Modifier.fillMaxWidth().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Avatar(initials, color)
            Spacer(Modifier.size(8.dp))
            Column {
                Text(
                    author,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
        Spacer(Modifier.size(6.dp))
        NostrRichText(content = content)
    }
}

@Composable
private fun MissingEventRow(eventId: String) {
    Text(
        "Event pending ${eventId.take(8)}",
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.fillMaxWidth().padding(12.dp),
    )
}

@Composable
private fun Avatar(initials: String, colorHex: String) {
    Surface(
        modifier = Modifier.size(36.dp).clip(CircleShape),
        color = parseHexColor(colorHex) ?: MaterialTheme.colorScheme.secondary,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(
                initials,
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

/** `#RRGGBB` / `RRGGBB` → Color; null on malformed (caller falls back). */
private fun parseHexColor(hex: String): Color? {
    val clean = hex.trim().removePrefix("#")
    if (clean.length != 6) return null
    val v = clean.toLongOrNull(16) ?: return null
    return Color(
        red = ((v shr 16) and 0xFF) / 255f,
        green = ((v shr 8) and 0xFF) / 255f,
        blue = (v and 0xFF) / 255f,
    )
}

private fun blockKey(index: Int, block: TimelineBlock): String {
    val ids = block.eventIds.joinToString(":")
    return ids.ifEmpty { "block-$index" }
}

private fun String.nonEmptyOrNull(): String? = if (isEmpty()) null else this
