package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.registry.NostrLoginBlock

/**
 * Gallery pages for the "auth" section — login-block showcase.
 *
 * This is the Android peer of the SwiftUI `AuthComponentPages.swift` in
 * `apps/nmp-gallery/ios`. It demonstrates the `login-block` registry
 * component (ADR-0048 Stage 2 — NIP-55 Amber sign-in) wired to the REAL
 * kernel flow: a tap dispatches `NmpApp.signinNip55`, Rust builds the
 * `get_public_key` capability request, the activity-registered
 * `ExternalSignerCapabilityBridge` fires the Intent, and the resulting
 * `signer_state` projection drives the inline status indicators.
 */
@Composable
fun AuthComponentPage(model: GalleryModel, componentId: String) {
    when (componentId) {
        "login-block" -> LoginBlockPage(model)
        else -> Text("Unknown auth component: $componentId")
    }
}

@Composable
private fun LoginBlockPage(model: GalleryModel) {
    val signerState by model.signerState.collectAsStateWithLifecycle()
    Column(modifier = Modifier.padding(16.dp)) {
        Text(
            text = "NostrLoginBlock",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "Detects installed Nostr signer apps (Amber) via PackageManager " +
                "and surfaces each as a one-tap sign-in option. Falls back to manual " +
                "key entry when no signers are installed. Tapping a signer card runs " +
                "the real NIP-55 get_public_key flow through the kernel capability " +
                "bridge (ADR-0048).",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(16.dp))
        // Live showcase: the REAL ADR-0048 flow. The tap reports user intent
        // to Rust (D7); Rust builds the request; the activity-registered
        // ExternalSignerCapabilityBridge executes it; signer_state renders
        // the round-trip inline. This is the surface the Stage-4 emulator
        // E2E drives.
        NostrLoginBlock(
            onSignerSelected = { signer -> model.signInWithAmber(signer) },
            onManualKey = {
                // Gallery showcase: the gallery has no nsec entry screen; a
                // real app navigates to its key-entry view here.
            },
            signerState = signerState,
        )
    }
}
