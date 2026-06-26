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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.dispatchWalletConnect
import org.nmp.android.dispatchWalletDisconnect

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
 *
 * ADR-0032 / #623: `walletLabel` and `walletTone` are pre-computed by Rust
 * (via [TypedWalletDecoder]) so this composable never branches on raw protocol
 * strings (thin-shell rule). `walletLabel` is rendered verbatim; `walletTone`
 * is mapped to a [Color] by [colorForTone] — the only presentation concern
 * remaining in this layer.
 */
@Composable
fun WalletScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val s by model.state.collectAsStateWithLifecycle()
    var nwcUri by remember { mutableStateOf("") }
    var isConnecting by remember { mutableStateOf(false) }

    // ADR-0032 / #623: bind pre-computed label and tone — no raw-string
    // branching in Kotlin (thin-shell rule). Both are null when no wallet
    // is configured on this snapshot tick.
    val walletLabel = s.projections?.walletLabel
    // Rust owns the connected decision (`WalletStatus.is_connected`); the shell
    // binds it verbatim instead of re-deriving from the tone discriminant (which
    // was a native branch on a Rust wire value; D7 / thin-shell). Mirrors iOS,
    // which gates on `status.isConnected`. `null` (no wallet projection) ⇒ false.
    val isConnected = s.projections?.walletIsConnected ?: false
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
                            // `walletLabel` is null only when no wallet is configured
                            // (snapshot carries no wallet projection at all). "Not
                            // connected" is the only display-layer default — tone
                            // remains null so `isConnected` stays false.
                            walletLabel ?: "Not connected",
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
                            model.dispatchWalletConnect(nwcUri)
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
 * Map a pre-computed `statusTone` string to a [Color].
 *
 * The tone vocabulary is `"active"` | `"warning"` | `"error"` | `"inactive"`
 * (ADR-0032 / #623). No other protocol-string knowledge lives here — the Rust
 * projection owns the mapping from wire status to tone (thin-shell rule).
 */
@Suppress("UnusedPrivateMember")
private fun colorForTone(tone: String?): Color = when (tone) {
    "active"  -> Color(0xFF4CAF50)   // green — mirrors ChirpColor.success
    "warning" -> Color(0xFFFFC107)   // amber — mirrors ChirpColor.zap
    "error"   -> Color(0xFFF44336)   // red   — mirrors ChirpColor.danger
    else      -> Color(0xFF9E9E9E)   // neutral grey
}
