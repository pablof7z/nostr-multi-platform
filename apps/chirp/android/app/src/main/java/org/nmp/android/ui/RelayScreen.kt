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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.KernelUpdate
import org.nmp.android.model.RelayStatus

/**
 * Relay management screen for Android Chirp — displays current relay
 * connections and allows adding/removing relays.
 *
 * Renders the [relayStatuses] from the kernel snapshot with per-relay
 * status indicators and remove buttons. Includes an add-relay form at the
 * bottom with URL and role fields.
 *
 * Routes relay operations through [KernelModel] which dispatches them as
 * actions (e.g., "nmp.relay.add", "nmp.relay.remove").
 */
@Composable
fun RelayScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val state by model.state.collectAsStateWithLifecycle()
    val relays = state.relayStatuses
    val isLoading = state.rev == 0L

    var showAddForm by remember { mutableStateOf(false) }
    var newRelayUrl by remember { mutableStateOf("") }
    var newRelayRole by remember { mutableStateOf("Read") }

    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            // Header
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Relays", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "${relays.size} relays",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            HorizontalDivider()

            // Relays list or loading state
            if (isLoading) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        CircularProgressIndicator()
                        Spacer(Modifier.size(16.dp))
                        Text("Loading relays…")
                    }
                }
            } else if (relays.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        "No relays configured",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxWidth().weight(1f)) {
                    items(relays, key = { "${it.role}:${it.relayUrl}" }) { relay ->
                        RelayRow(relay) {
                            model.removeRelay(relay.relayUrl)
                        }
                        HorizontalDivider()
                    }
                }
            }

            // Add relay form
            HorizontalDivider()
            AddRelayForm(
                url = newRelayUrl,
                onUrlChange = { newRelayUrl = it },
                role = newRelayRole,
                onRoleChange = { newRelayRole = it },
                onAdd = {
                    if (newRelayUrl.isNotBlank()) {
                        model.addRelay(newRelayUrl, newRelayRole)
                        newRelayUrl = ""
                        newRelayRole = "Read"
                    }
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
            )
        }
    }
}

/**
 * One relay row: URL, role badge, status indicator, and remove button.
 *
 * Status indicator colors:
 * - Green: "Connected"
 * - Yellow: "Connecting" or "Reconnecting"
 * - Red: "Disconnected" or "Failed"
 * - Gray: Other states
 */
@Composable
private fun RelayRow(
    relay: RelayStatus,
    onRemove: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(12.dp)
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                // URL with monospace font for readability
                Text(
                    relay.relayUrl,
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Medium,
                    ),
                    modifier = Modifier.fillMaxWidth(0.85f),
                    maxLines = 1,
                )
                Spacer(Modifier.size(4.dp))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // Role badge
                    Surface(
                        color = MaterialTheme.colorScheme.secondaryContainer,
                        shape = MaterialTheme.shapes.small,
                        modifier = Modifier.padding(0.dp),
                    ) {
                        Text(
                            relay.role,
                            style = MaterialTheme.typography.labelSmall,
                            modifier = Modifier.padding(4.dp, 2.dp),
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                    // Status indicator + label
                    val (statusColor, statusLabel) = statusColors(relay.connection)
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Surface(
                            color = statusColor,
                            shape = MaterialTheme.shapes.small,
                            modifier = Modifier.size(8.dp),
                        ) {}
                        Text(
                            statusLabel,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Spacer(Modifier.size(6.dp))
                Text(
                    "Subscriptions: ${relay.activeWireSubscriptions} | Reconnects: ${relay.reconnectCount}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = onRemove) {
                Icon(Icons.Filled.Delete, contentDescription = "Remove relay")
            }
        }
    }
}

/** Determine status indicator color and label based on connection state. */
private fun statusColors(connection: String): Pair<Color, String> {
    return when (connection.lowercase()) {
        "connected" -> Color(0xFF4CAF50) to "Connected"
        "connecting", "reconnecting" -> Color(0xFFFFC107) to "Connecting"
        "disconnected", "failed" -> Color(0xFFF44336) to "Disconnected"
        else -> Color(0xFF9E9E9E) to connection
    }
}

/**
 * Add relay form: URL text field, role dropdown, and add button.
 */
@Composable
private fun AddRelayForm(
    url: String,
    onUrlChange: (String) -> Unit,
    role: String,
    onRoleChange: (String) -> Unit,
    onAdd: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var expandedRoleMenu by remember { mutableStateOf(false) }
    val roleOptions = listOf("Read", "Write", "ReadWrite")

    Column(modifier.fillMaxWidth()) {
        Text(
            "Add Relay",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
        )
        Spacer(Modifier.size(12.dp))
        TextField(
            value = url,
            onValueChange = onUrlChange,
            label = { Text("Relay URL") },
            placeholder = { Text("wss://relay.example.com") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Spacer(Modifier.size(12.dp))
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) {
                Button(
                    onClick = { expandedRoleMenu = !expandedRoleMenu },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(role)
                }
                DropdownMenu(
                    expanded = expandedRoleMenu,
                    onDismissRequest = { expandedRoleMenu = false },
                    modifier = Modifier.fillMaxWidth(0.5f),
                ) {
                    roleOptions.forEach { opt ->
                        DropdownMenuItem(
                            text = { Text(opt) },
                            onClick = {
                                onRoleChange(opt)
                                expandedRoleMenu = false
                            },
                        )
                    }
                }
            }
            Button(
                onClick = onAdd,
                enabled = url.isNotBlank(),
                modifier = Modifier.weight(0.8f),
            ) {
                Text("Add")
            }
        }
    }
}
