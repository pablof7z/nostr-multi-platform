package org.nmp.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.ExternalSignerCapabilityBridge
import org.nmp.android.KernelModel
import org.nmp.android.NostrSignerInfo
import org.nmp.android.createAccount
import org.nmp.android.detectInstalledSigners
import org.nmp.android.signInBunker
import org.nmp.android.signInNsec
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
/**
 * Callback interface the host activity implements to wire the
 * [ExternalSignerCapabilityBridge] into [SignInScreen].
 *
 * The host is responsible for launching the Activity Result and routing
 * the response back to Rust. The screen itself triggers the start; the
 * bridge owns the dispatch (D7 — no sign-in logic in the screen).
 */
interface SignInAmberDelegate {
    /** Initiate the NIP-55 `get_public_key` flow for the given signer. */
    fun signInWithAmber(signer: NostrSignerInfo)
}

@Composable
fun SignInScreen(
    model: KernelModel,
    amberDelegate: SignInAmberDelegate? = null,
    modifier: Modifier = Modifier,
) {
    var nsecSecret by remember { mutableStateOf("") }
    var displayName by remember { mutableStateOf("") }
    var bunkerUri by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf("") }
    // ADR-0048 D6 (generalises V-14 / #963): unified remote-signer health.
    // Null while no remote-signer session is active (local-key accounts).
    val signerState by model.signerState.collectAsStateWithLifecycle()

    // ADR-0048 Stage 2: detect installed NIP-55 signer apps.
    // Detection result drives the "Sign in with Amber" affordance; the list
    // is reported to the screen — Rust owns the gating decision (D7).
    val context = LocalContext.current
    var availableSigners by remember { mutableStateOf<List<NostrSignerInfo>>(emptyList()) }
    LaunchedEffect(Unit) {
        availableSigners = detectInstalledSigners(context.packageManager)
    }

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

        // ADR-0048 Stage 2: Sign in with Amber (NIP-55) — shown only when at
        // least one NIP-55 signer app is installed (D7: Kotlin reports; Rust gates).
        if (availableSigners.isNotEmpty()) {
            availableSigners.forEach { signer ->
                AmberSignerCard(
                    signer = signer,
                    signerState = signerState?.takeIf { it.signerKind == "nip55" },
                    onClick = {
                        amberDelegate?.signInWithAmber(signer)
                    },
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
        }

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
 * ADR-0048 Stage 2: Amber (NIP-55) sign-in card in the Chirp sign-in screen.
 *
 * Consumes the same [NostrSignerInfo] + [SignerState] as the gallery's
 * `NostrLoginBlock` component. Visual states are identical:
 *  - `isAwaitingApproval` → amber spinner + "Waiting for approval…"
 *  - `isReady` → green border + "Connected"
 *  - `isFailed` / `isUnavailable` → red border + error text
 *  - null signer state → default border + "Sign in with {name}"
 *
 * testTags are present for the Stage-4 emulator E2E.
 */
@Composable
private fun AmberSignerCard(
    signer: NostrSignerInfo,
    signerState: SignerState?,
    onClick: () -> Unit,
) {
    val isInProgress = signerState?.isAwaitingApproval == true || signerState?.isReconnecting == true
    val isDegraded = signerState?.isFailed == true || signerState?.isUnavailable == true
    val isReady = signerState?.isReady == true

    val borderColor: Color = when {
        isDegraded -> MaterialTheme.colorScheme.error
        isInProgress -> Color(0xFFF59E0B)
        isReady -> Color(0xFF22C55E)
        else -> MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)
    }

    // #1493 P9: when a session exists, render the shell-derived `statusLabel`
    // (TypedSignerStateDecoder.deriveStatusLabel from the raw `state` token);
    // the no-session affordance is the only local copy.
    val subtitleText: String = signerState?.statusLabel?.takeIf { it.isNotEmpty() }
        ?: "Sign in with ${signer.displayName}"

    OutlinedCard(
        modifier = Modifier
            .fillMaxWidth()
            .testTag("amber_signer_card")
            .semantics { contentDescription = "Sign in with ${signer.displayName}" }
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(8.dp),
        border = BorderStroke(1.dp, borderColor),
        colors = CardDefaults.outlinedCardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                imageVector = Icons.Default.Person,
                contentDescription = null,
                modifier = Modifier.size(24.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = signer.displayName,
                    style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                )
                Text(
                    text = subtitleText,
                    style = MaterialTheme.typography.bodySmall,
                    color = when {
                        isDegraded -> MaterialTheme.colorScheme.error
                        isInProgress -> Color(0xFFF59E0B)
                        isReady -> Color(0xFF22C55E)
                        else -> MaterialTheme.colorScheme.onSurfaceVariant
                    },
                )
            }
            if (isInProgress) {
                CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    color = Color(0xFFF59E0B),
                    strokeWidth = 2.dp,
                )
            } else {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.KeyboardArrowRight,
                    contentDescription = null,
                    modifier = Modifier.size(20.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
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
 * "External signer" for NIP-55). Rust pre-computes the `is*` flags (ADR-0032
 * relay_diagnostics pattern), so Compose never string-compares `state` for
 * control flow. Per #1493 P9 the English `statusLabel`/`statusTone` strings are
 * shell-derived (TypedSignerStateDecoder) from the raw `state` token, not on the
 * wire.
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
    // #1493 P9: bind the shell-derived label verbatim and map the shell-derived
    // tone → colour (both from TypedSignerStateDecoder, off the raw `state`
    // token) — no `when` on `state` for control flow remains here.
    val statusLabel = signerState.statusLabel
    val statusColor: Color = when (signerState.statusTone) {
        "error" -> MaterialTheme.colorScheme.error
        "warning" -> Color(0xFFF59E0B) // amber-400
        "active" -> Color(0xFF22C55E) // green-500
        else -> Color(0xFF9E9E9E) // neutral grey
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
