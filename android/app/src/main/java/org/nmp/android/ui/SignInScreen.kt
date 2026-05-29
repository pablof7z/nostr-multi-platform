package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import org.nmp.android.KernelModel

/**
 * Sign-in screen for Android Chirp app. Provides three authentication paths:
 * 1. Sign in with nsec (hex secret or bech32 private key)
 * 2. Create a local account with a display name
 * 3. Connect to a Bunker relay (NIP-46 remote signer)
 *
 * All actions route through the shared KernelModel: signInNsec, createAccount,
 * and dispatchAction for Bunker. No local KernelBridge instantiation.
 */
@Composable
fun SignInScreen(model: KernelModel, modifier: Modifier = Modifier) {
    var nsecSecret by remember { mutableStateOf("") }
    var displayName by remember { mutableStateOf("") }
    var bunkerRelayUrl by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf("") }

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
                    value = bunkerRelayUrl,
                    onValueChange = { bunkerRelayUrl = it },
                    label = { Text("Relay URL") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                Button(
                    onClick = {
                        if (bunkerRelayUrl.isBlank()) {
                            errorMessage = "Please enter a relay URL"
                        } else {
                            val actionJson = """{"ConnectBunker":"${escapeJson(bunkerRelayUrl)}"}"""
                            model.dispatchAction("nmp.sign_in", actionJson)
                            bunkerRelayUrl = ""
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = bunkerRelayUrl.isNotBlank(),
                ) {
                    Text("Connect")
                }
            }
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
 * Escape special characters in JSON strings: backslash, quote, newline, carriage
 * return, and tab. Mirrors the iOS KernelModel pattern.
 */
private fun escapeJson(s: String): String {
    return s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
}
