package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.registry.NostrLoginBlock

/**
 * Gallery pages for the "auth" section — login-block showcase.
 *
 * This is the Android peer of the SwiftUI `AuthComponentPages.swift` in
 * `apps/nmp-gallery/ios`. It demonstrates the `login-block` registry
 * component (ADR-0048 Stage 2 — NIP-55 Amber sign-in).
 */
@Composable
fun AuthComponentPage(model: GalleryModel, componentId: String) {
    when (componentId) {
        "login-block" -> LoginBlockPage()
        else -> Text("Unknown auth component: $componentId")
    }
}

@Composable
private fun LoginBlockPage() {
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
                "key entry when no signers are installed.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.height(16.dp))
        // Live showcase: detection + callbacks stubbed for the gallery.
        // A real app wires onSignerSelected to ExternalSignerCapabilityBridge.
        NostrLoginBlock(
            onSignerSelected = { signer ->
                // Gallery showcase: no-op (no kernel instance for sign-in).
                // A real app would call:
                //   bridge.handle(ExternalSignerCapabilityBridge.buildGetPublicKeyRequest(signer))
            },
            onManualKey = {
                // Gallery showcase: no-op.
                // A real app would navigate to an nsec entry screen.
            },
        )
    }
}
