package org.nmp.gallery.gallery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
 * The avatar page passes only a pubkey; `NostrAvatar` claims/releases its own
 * profile projection through `LocalNostrProfileHost`.
 */
@Composable
fun UserComponentPage(
    model: GalleryModel,
    componentId: String,
) {
    val profiles by model.profileMap.collectAsStateWithLifecycle()
    val showcase = model.showcase
    val pubkey = showcase.profile.pubkeyHex

    val profile = profiles[pubkey] ?: ProfileWire(
        pubkey = pubkey,
        npub = showcase.profile.npub,
        npubShort = showcase.profile.npubShort,
    )

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
        UserComponentBody(componentId = componentId, pubkey = pubkey, profile = profile)
    }
}

@Composable
private fun UserComponentBody(componentId: String, pubkey: String, profile: ProfileWire) {
    when (componentId) {
        "user-avatar" -> NostrAvatar(pubkey = pubkey, size = 80.dp)
        "user-name" -> NostrProfileName(profile = profile)
        "user-nip05" -> NostrNip05Badge(profile = profile)
        "user-npub" -> NostrNpubChip(profile = profile)
        "user-card" -> NostrUserCard(profile = profile)
        else -> Text("Unknown user component: $componentId")
    }
}

private fun labelFor(componentId: String): String = when (componentId) {
    "user-avatar" -> "NostrAvatar(pubkey)"
    "user-name" -> "NostrProfileName (live profile)"
    "user-nip05" -> "NostrNip05Badge (live profile)"
    "user-npub" -> "NostrNpubChip (live profile)"
    "user-card" -> "NostrUserCard (live profile)"
    else -> componentId
}
