package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.gallery.bridge.GalleryModel
import org.nmp.gallery.registry.NostrAvatar
import org.nmp.gallery.registry.NostrNip05Badge
import org.nmp.gallery.registry.NostrNpubChip
import org.nmp.gallery.registry.NostrProfileName
import org.nmp.gallery.registry.NostrUserCard
import org.nmp.gallery.registry.ProfileWire

/**
 * Render the user-* family of registry components against real kind:0
 * profile data fetched by the NMP kernel.
 *
 * The model lifecycle owns the kernel claim; this composable re-asserts
 * the claim for its own consumer id so multiple screens can independently
 * track the same pubkey without stomping on each other.
 */
@Composable
fun UserComponentPage(
    model: GalleryModel,
    componentId: String,
) {
    val profiles by model.profileMap.collectAsStateWithLifecycle()
    val pubkey = remember { GalleryModel.DEMO_PUBKEY }
    val consumerId = remember(componentId) { "gallery-page:$componentId" }

    DisposableEffect(pubkey, consumerId) {
        model.claimProfile(pubkey, consumerId)
        onDispose { model.releaseProfile(pubkey, consumerId) }
    }

    val profile = profiles[pubkey]
    if (profile == null) {
        ProfileLoading()
        return
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = labelFor(componentId),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        UserComponentBody(componentId = componentId, profile = profile)
    }
}

@Composable
private fun UserComponentBody(componentId: String, profile: ProfileWire) {
    when (componentId) {
        "user-avatar" -> NostrAvatar(profile = profile, size = 80.dp)
        "user-name" -> NostrProfileName(profile = profile)
        "user-nip05" -> NostrNip05Badge(profile = profile)
        "user-npub" -> NostrNpubChip(profile = profile)
        "user-card" -> NostrUserCard(profile = profile)
        else -> Text("Unknown user component: $componentId")
    }
}

@Composable
private fun ProfileLoading() {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            CircularProgressIndicator(modifier = Modifier.size(32.dp))
            Text(
                text = "Loading profile…",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun labelFor(componentId: String): String = when (componentId) {
    "user-avatar" -> "NostrAvatar (live profile)"
    "user-name" -> "NostrProfileName (live profile)"
    "user-nip05" -> "NostrNip05Badge (live profile)"
    "user-npub" -> "NostrNpubChip (live profile)"
    "user-card" -> "NostrUserCard (live profile)"
    else -> componentId
}
