package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.SignerState

/**
 * Sign-in screen for Android Chirp app. Provides two authentication paths:
 * 1. Sign in with nsec (hex secret or bech32 private key)
 * 2. Create a local account with a display name
 * 3. Connect to a bunker URI (NIP-46 remote signer)
 *
 * All actions route through the shared KernelModel: signInNsec, createAccount,
 * and signInBunker. No local KernelBridge instantiation.
 */
@Composable
fun SignInScreen(model: KernelModel, modifier: Modifier = Modifier) {
    var nsecSecret by remember { mutableStateOf("") }
    var displayName by remember { mutableStateOf("") }
    var bunkerUri by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf("") }
    // ADR-0048 D6 (generalises V-14 / #963): unified remote-signer health.
    // Null while no remote-signer session is active (local-key accounts).
    val signerState by model.signerState.collectAsStateWithLifecycle()

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Header
        Text(
            "Chirp Sign In",
            style = MaterialTheme.typography.headlineLarge,
            modifier = Modifier.padding(top = 32.dp),
        )
        Text(
            "Choose how to sign in to your Nostr account",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(Modifier.size(24.dp))

        // Sign In with Nsec Section
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    "Sign In with Private Key",
                    style = MaterialTheme.typography.titleMedium,
                )
                OutlinedTextField(
                    value = nsecSecret,
                    onValueChange = { nsecSecret = it },
                    label = { Text("nsec or hex secret") },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Button(
                    onClick = {
                        if (nsecSecret.isBlank()) {
                            errorMessage = "Please enter a private key"
                        } else {
                            model.signInNsec(nsecSecret)
                            nsecSecret = ""
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = nsecSecret.isNotBlank(),
                ) {
                    Text("Sign In")
                }
            }
        }

        HorizontalDivider(Modifier.padding(vertical = 8.dp))

        // Create Local Account Section
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    "Create Local Account",
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    "Generate a new account on this device",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = displayName,
                    onValueChange = { displayName = it },
                    label = { Text("Display name") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Button(
                    onClick = {
                        model.createAccount(displayName)
                        displayName = ""
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Create Account")
                }
            }
        }

        HorizontalDivider(Modifier.padding(vertical = 8.dp))

        // Connect Bunker (NIP-46) Section
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    "Connect Bunker",
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    "Sign in using a remote signer (NIP-46)",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = bunkerUri,
                    onValueChange = { bunkerUri = it },
                    label = { Text("bunker:// URI") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                Button(
                    onClick = {
                        if (bunkerUri.isBlank()) {
                            errorMessage = "Please enter a bunker URI"
                        } else {
                            model.signInBunker(bunkerUri.trim())
                            bunkerUri = ""
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = bunkerUri.isNotBlank(),
                ) {
                    Text("Connect")
                }
            }
        }

        // ADR-0048 D6 (generalises V-14 / #963): remote-signer health badge —
        // only shown when a remote-signer session (NIP-46 bunker or NIP-55
        // Amber) is active. `isReady` → green; `isAwaitingApproval` /
        // `isReconnecting` → amber spinner; `isUnavailable` / `isFailed` →
        // red. Rust pre-computes all flags (ADR-0032).
        signerState?.let { state ->
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            SignerStateRow(
                signerState = state,
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.size(16.dp))

        // Error Message Display
        if (errorMessage.isNotEmpty()) {
            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 8.dp),
                shape = RoundedCornerShape(8.dp),
            ) {
                Text(
                    errorMessage,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(12.dp),
                )
            }
        }

        Spacer(Modifier.size(32.dp))
    }
}

/**
 * ADR-0048 D6 (generalises V-14 / #963): inline remote-signer health indicator.
 *
 * Rendered only when `signerState` is non-null (i.e. a remote-signer session —
 * NIP-46 bunker or NIP-55 Amber — is active). Visual states:
 *  - `isReady` → green dot + "Connected"
 *  - `isAwaitingApproval` → amber spinner + "Waiting for approval…" (approve
 *    in the signer app)
 *  - `isReconnecting` → amber spinner + "Reconnecting…" (wait)
 *  - `isUnavailable` → red warning + "Signer unavailable" (re-auth)
 *  - `isFailed` → red warning + "Connection failed" (re-auth)
 *
 * The row label is picked from `signerKind` ("Signer relay" for NIP-46,
 * "External signer" for NIP-55). Rust pre-computes every flag (ADR-0032
 * relay_diagnostics pattern); Compose renders verbatim — no string-compare on
 * `signerState.state`.
 */
@Composable
private fun SignerStateRow(
    signerState: SignerState,
    modifier: Modifier = Modifier,
) {
    // Degraded-terminal grouping (red, prompt re-auth) and transient
    // in-progress grouping (amber spinner) — both pre-computed flags.
    val isDegradedTerminal = signerState.isFailed || signerState.isUnavailable
    val isInProgress = signerState.isAwaitingApproval || signerState.isReconnecting
    val rowLabel = if (signerState.signerKind == "nip55") "External signer" else "Signer relay"
    val statusLabel = when {
        signerState.isUnavailable -> "Signer unavailable"
        signerState.isFailed -> "Connection failed"
        signerState.isAwaitingApproval -> "Waiting for approval…"
        signerState.isReconnecting -> "Reconnecting…"
        else -> "Connected"
    }
    val statusColor: Color = when {
        isDegradedTerminal -> MaterialTheme.colorScheme.error
        isInProgress -> Color(0xFFF59E0B) // amber-400
        else -> Color(0xFF22C55E) // green-500
    }

    Card(
        modifier = modifier.semantics {
            contentDescription = "$rowLabel: $statusLabel"
        },
        shape = RoundedCornerShape(8.dp),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (isInProgress) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(16.dp),
                        color = statusColor,
                        strokeWidth = 2.dp,
                    )
                } else {
                    // Use a filled circle indicator via MaterialTheme icon
                    Text(
                        text = "●", // BULLET / filled circle
                        color = statusColor,
                        style = MaterialTheme.typography.titleMedium,
                    )
                }
                Text(
                    text = "$rowLabel: $statusLabel",
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (isDegradedTerminal) {
                        MaterialTheme.colorScheme.error
                    } else {
                        MaterialTheme.colorScheme.onSurface
                    },
                )
            }
            signerState.reason?.takeIf { it.isNotEmpty() }?.let { reason ->
                Text(
                    text = reason,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (isDegradedTerminal) {
                        MaterialTheme.colorScheme.error
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                )
            }
        }
    }
}
