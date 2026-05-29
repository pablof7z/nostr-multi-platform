package org.nmp.gallery.registry

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

data class NostrRelayEditRow(
    val url: String,
    val role: String,
    val roleLabel: String,
    val roleTint: String,
)

@Composable
fun NostrRelayList(
    relays: List<NostrRelayEditRow>,
    connectionStatus: Map<String, String>,
    modifier: Modifier = Modifier,
) {
    if (relays.isEmpty()) {
        Text(
            text = "No relays configured",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = modifier.padding(16.dp),
        )
        return
    }
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(0.dp)) {
        relays.forEach { relay ->
            RelayRow(relay = relay, status = connectionStatus[relay.url])
        }
    }
}

@Composable
private fun RelayRow(relay: NostrRelayEditRow, status: String?) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Surface(
            modifier = Modifier.size(10.dp).clip(CircleShape),
            color = statusColor(status),
        ) {}
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = relay.url,
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        RoleBadge(label = relay.roleLabel, tint = relay.roleTint)
    }
}

@Composable
private fun RoleBadge(label: String, tint: String) {
    val bgColor = when (tint) {
        "success" -> Color(0xFF166534)
        "info" -> Color(0xFF1E40AF)
        else -> MaterialTheme.colorScheme.secondaryContainer
    }
    val textColor = when (tint) {
        "success" -> Color(0xFF86EFAC)
        "info" -> Color(0xFF93C5FD)
        else -> MaterialTheme.colorScheme.onSecondaryContainer
    }
    Surface(color = bgColor, shape = MaterialTheme.shapes.extraSmall) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = textColor,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

private fun statusColor(status: String?): Color = when (status) {
    "connected" -> Color(0xFF22C55E)
    "connecting" -> Color(0xFFF59E0B)
    "error" -> Color(0xFFEF4444)
    else -> Color(0xFF6B7280)
}
