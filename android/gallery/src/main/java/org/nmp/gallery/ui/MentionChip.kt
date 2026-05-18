package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.EmbedEntry

/**
 * Profile mention chip — Compose port of Swift `MentionChip`. Resolves
 * kind:0 from the relay-free embed store; falls back to a D1 deterministic
 * identicon + truncated npub when the profile is absent or has no picture.
 */
@Composable
fun MentionChip(
    pubkey: String,
    entry: EmbedEntry?,
    modifier: Modifier = Modifier,
) {
    val name = entry?.profileName
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        modifier = modifier
            .clip(RoundedCornerShape(percent = 50))
            .background(Indigo.copy(alpha = 0.14f))
            .padding(horizontal = 8.dp, vertical = 4.dp),
    ) {
        Identicon(seed = pubkey, modifier = Modifier.size(22.dp))
        Column(verticalArrangement = Arrangement.spacedBy(0.dp)) {
            Text(
                text = if (name != null) "@$name" else "@npub1${pubkey.take(6)}…",
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
            )
            if (name == null) {
                Text(
                    text = "npub1${pubkey.take(8)}…",
                    style = MaterialTheme.typography.labelSmall.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

internal val Indigo = Color(0xFF5856D6) // matches SwiftUI Color.indigo
internal val SwiftBlue = Color(0xFF007AFF) // matches SwiftUI Color.blue
internal val SwiftOrange = Color(0xFFFF9500) // matches SwiftUI Color.orange
internal val SwiftAccent = Color(0xFF007AFF) // accentColor default = systemBlue
internal val SwiftTeal = Color(0xFF30B0C7) // matches SwiftUI Color.teal
internal val SwiftGreen = Color(0xFF34C759) // matches SwiftUI Color.green
internal val SwiftRed = Color(0xFFFF3B30) // matches SwiftUI Color.red
internal val SwiftPurple = Color(0xFFAF52DE) // matches SwiftUI Color.purple
