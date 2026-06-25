package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ProfileProjectionEntry

/**
 * kind:0 profile-metadata embed (#984). Android peer of iOS
 * `DefaultProfileRenderer`. Renders the resolved display name + about line with
 * the author avatar. All fields arrive resolved from the kernel's embed
 * resolver (no kind:0 parsing here).
 */
@Composable
fun ProfileEmbedView(profile: ProfileProjectionEntry) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = profile.pubkey,
            authorDisplayName = profile.displayName,
            caption = profile.nip05?.takeIf { it.isNotEmpty() } ?: "profile",
            avatarConsumerId = "embed-profile-${profile.pubkey}",
        )
        profile.about?.takeIf { it.isNotEmpty() }?.let { about ->
            Text(
                about,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
