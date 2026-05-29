package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.bridge.GalleryShowcaseReferences
import org.nmp.gallery.registry.NostrRelayEditRow
import org.nmp.gallery.registry.NostrRelayList

@Composable
fun RelayComponentPage(model: GalleryModel, componentId: String) {
    when (componentId) {
        "relay-list" -> RelayListPage(showcase = model.showcase)
        else -> Text("Unknown relay component: $componentId")
    }
}

@Composable
private fun RelayListPage(showcase: GalleryShowcaseReferences) {
    val relayRows = showcase.relays.map { relay ->
        NostrRelayEditRow(
            url = relay.url,
            role = relay.role,
            roleLabel = roleLabel(relay.role),
            roleTint = roleTint(relay.role),
        )
    }
    val statusesByRelay = showcase.relays.mapIndexed { index, relay ->
        relay.url to if (index == 0) "connecting" else "connected"
    }.toMap()

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            text = "NostrRelayList",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        NostrRelayList(relays = relayRows, connectionStatus = statusesByRelay)
    }
}

private fun roleLabel(role: String): String = when {
    "both" in role && "indexer" in role -> "Both + Indexer"
    "indexer" in role -> "Indexer"
    "both" in role -> "Both"
    else -> role.replaceFirstChar { it.uppercaseChar() }
}

private fun roleTint(role: String): String = when {
    "both" in role -> "success"
    "indexer" in role -> "info"
    else -> "accent"
}
