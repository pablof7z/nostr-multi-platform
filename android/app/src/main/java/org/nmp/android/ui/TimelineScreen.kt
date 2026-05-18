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
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.TimelineItem

/**
 * Live kind:1 feed straight from the kernel snapshot — Android peer of iOS
 * `TimelineView`. Renders verbatim; no sorting/derivation (D8).
 */
@Composable
fun TimelineScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val s by model.state.collectAsStateWithLifecycle()

    Column(modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text("Pulse", style = MaterialTheme.typography.headlineSmall)
            Text("rev ${s.rev}", style = MaterialTheme.typography.labelSmall)
        }
        HorizontalDivider()
        if (s.items.isEmpty()) {
            Placeholder(s.testNpub)
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(s.items, key = { it.id }) { item ->
                    NoteRow(item)
                    HorizontalDivider()
                }
            }
        }
    }
}

@Composable
private fun Placeholder(testNpub: String) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            CircularProgressIndicator()
            Spacer(Modifier.size(16.dp))
            Text("Waiting for kernel snapshot…")
            Spacer(Modifier.size(8.dp))
            Text(
                "Bootstrap pubkey: $testNpub",
                style = MaterialTheme.typography.bodySmall,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

@Composable
private fun NoteRow(item: TimelineItem) {
    Column(Modifier.fillMaxWidth().padding(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Avatar(item.authorAvatarInitials, item.authorAvatarColor)
            Spacer(Modifier.size(8.dp))
            Column {
                Text(
                    item.authorDisplay,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    item.createdAtDisplay,
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
        Spacer(Modifier.size(6.dp))
        Text(
            item.contentPreview.ifEmpty { item.content },
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 8,
            overflow = TextOverflow.Ellipsis,
        )
    }
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
