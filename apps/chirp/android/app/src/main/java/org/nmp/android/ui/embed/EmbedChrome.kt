package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.android.components.NostrAvatar

/**
 * Shared byline header for the embed kind composables (#984). Paints the
 * author's [NostrAvatar] (which self-claims the kind:0 via the profile host)
 * plus a precomputed display label and a kind/time caption.
 *
 * The author display name arrives already-resolved in the typed projection
 * ([authorDisplayName] from the kernel's embed resolver); this only chooses
 * between that label and a short-hex fallback — no protocol logic.
 */
@Composable
internal fun EmbedByline(
    authorPubkey: String,
    authorDisplayName: String?,
    caption: String,
    avatarConsumerId: String,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        NostrAvatar(
            pubkey = authorPubkey,
            size = 28.dp,
            consumerId = avatarConsumerId,
        )
        Spacer(Modifier.size(8.dp))
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(
                authorDisplayName?.takeIf { it.isNotEmpty() } ?: shortHex(authorPubkey),
                style = MaterialTheme.typography.labelLarge,
                fontWeight = FontWeight.Bold,
            )
            Text(
                caption,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Truncate a hex pubkey/event-id for display. Mirrors iOS `shortHex`. */
internal fun shortHex(value: String): String {
    if (value.length <= 16) return value.ifEmpty { "unknown" }
    return "${value.take(8)}…${value.takeLast(8)}"
}
