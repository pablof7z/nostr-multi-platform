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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel

/**
 * Wallet (NIP-47 / NWC) connection screen for Android Chirp.
 *
 * Displays:
 * - Current wallet connection status
 * - NWC URI input field
 * - Connect button (routes through dispatch_action("nmp.wallet.connect", ...))
 * - Disconnect button (routes through dispatch_action("nmp.wallet.disconnect", ...))
 * - Balance display when connected
 *
 * Material3 styling, mirrors TimelineScreen patterns.
 */
@Composable
fun WalletScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val s by model.state.collectAsStateWithLifecycle()
    var nwcUri by remember { mutableStateOf("") }
    var isConnecting by remember { mutableStateOf(false) }

    // Wallet status from snapshot (if available)
    val walletStatus = s.projections?.walletStatus ?: ""
    val isConnected = walletStatus.equals("connected", ignoreCase = true)
    val balance = s.projections?.walletBalance ?: ""

    Box(modifier.fillMaxSize()) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(16.dp),
            verticalArrangement = Arrangement.Top,
        ) {
            // Header
            Text(
                "Wallet",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(bottom = 24.dp),
            )

            // Status card
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 16.dp),
                color = if (isConnected) {
                    MaterialTheme.colorScheme.tertiaryContainer
                } else {
                    MaterialTheme.colorScheme.surfaceVariant
                },
                shape = MaterialTheme.shapes.medium,
            ) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(
                            "Wallet Status",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.size(4.dp))
                        Text(
                            if (isConnected) "Connected" else "Not connected",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            color = if (isConnected) {
                                MaterialTheme.colorScheme.onTertiaryContainer
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                    }
                    if (isConnected) {
                        Icon(
                            Icons.Filled.Check,
                            contentDescription = "Connected",
                            tint = MaterialTheme.colorScheme.onTertiaryContainer,
                            modifier = Modifier.size(24.dp),
                        )
                    } else {
                        Icon(
                            Icons.Filled.Close,
                            contentDescription = "Not connected",
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.size(24.dp),
                        )
                    }
                }
            }

            // Balance display (when connected)
            if (isConnected && balance.isNotEmpty()) {
                Surface(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 16.dp),
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    shape = MaterialTheme.shapes.medium,
                ) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .padding(16.dp),
                    ) {
                        Text(
                            "Balance",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                        Spacer(Modifier.size(4.dp))
                        Text(
                            balance,
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                }
            }

            // Divider or spacing
            Spacer(Modifier.size(24.dp))

            // NWC URI input (only show when not connected)
            if (!isConnected) {
                Text(
                    "Nostr Wallet Connect",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
                TextField(
                    value = nwcUri,
                    onValueChange = { nwcUri = it },
                    label = { Text("nostr+walletconnect://...") },
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 16.dp),
                    maxLines = 3,
                    singleLine = false,
                )

                // Connect button
                Button(
                    onClick = {
                        if (nwcUri.isNotBlank()) {
                            isConnecting = true
                            val actionJson =
                                """{"Connect":{"uri":"${escapeJsonString(nwcUri)}"}}"""
                            model.dispatchWalletConnect(actionJson)
                            // Reset UI after a brief delay
                            isConnecting = false
                            nwcUri = ""
                        }
                    },
                    enabled = nwcUri.isNotBlank() && !isConnecting,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (isConnecting) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            color = Color.White,
                            strokeWidth = 2.dp,
                        )
                        Spacer(Modifier.size(8.dp))
                    }
                    Text("Connect Wallet")
                }
            } else {
                // Disconnect button (only show when connected)
                Button(
                    onClick = {
                        model.dispatchWalletDisconnect()
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Disconnect Wallet")
                }
            }
        }
    }
}

/**
 * Escape JSON string special characters.
 * Mirrors the escapeJson pattern from TimelineScreen.
 */
private fun escapeJsonString(s: String): String {
    return s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
}
