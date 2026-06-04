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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.ProfileCard

/**
 * Author/profile detail screen — Jetpack Compose peer of iOS `ProfileView`.
 *
 * Renders an author's profile header (avatar, display name, pubkey), claims
 * the profile with the kernel for demand-driven kind:0 fetching, and displays
 * the author's flat feed from `nmp.feed.author.<pubkey>`.
 *
 * Thin-shell rule: Rust owns author-feed membership; Compose renders raw
 * projection fields and applies presentation formatting locally.
 */
@Composable
fun ProfileScreen(
    pubkey: String,
    model: KernelModel,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val profileConsumerId = "profile_screen-$pubkey"
    DisposableEffect(pubkey) {
        model.openAuthor(pubkey)
        model.claimProfile(pubkey, profileConsumerId)
        onDispose {
            model.closeAuthor(pubkey)
            model.releaseProfile(pubkey, profileConsumerId)
        }
    }

    val snapshot by model.state.collectAsStateWithLifecycle()

    val projections = snapshot.projections
    val cards = projections?.flatFeeds?.get("nmp.feed.author.$pubkey")?.cards ?: emptyList()
    val cardLookup = cards.associate { it.card.id to it.card }

    val profileCard: ProfileCard? = projections
        ?.claimedProfiles
        ?.get(pubkey)
        ?: projections?.resolvedProfiles?.get(pubkey)
    val resolvedProfiles = if (profileCard != null) {
        (projections?.resolvedProfiles ?: emptyMap()) + (pubkey to profileCard)
    } else {
        projections?.resolvedProfiles ?: emptyMap()
    }

    val shortPubkey = abbreviateMiddle(pubkey.ifEmpty { "unknown" }, prefix = 8, suffix = 8)
    val npubLabel = profileCard
        ?.npub
        ?.takeIf { it.isNotEmpty() }
        ?.let { abbreviateMiddle(it, prefix = 10, suffix = 6) }
        ?: shortPubkey

    val displayName = profileCard?.displayName?.takeIf { it.isNotEmpty() } ?: shortPubkey
    val initials = displayName.take(2).uppercase()
    val noteCount = cards.size

    CompositionLocalProvider(
        LocalResolvedProfiles provides resolvedProfiles,
    ) {
        Box(modifier.fillMaxSize()) {
            Column(Modifier.fillMaxSize()) {
                // Header: back button + title
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back to timeline",
                        )
                    }
                    Text(
                        "Profile",
                        style = MaterialTheme.typography.headlineSmall,
                        modifier = Modifier.weight(1f),
                    )
                    Spacer(Modifier.size(40.dp))
                }

                HorizontalDivider()

                // Profile header section
                Column(
                    Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Surface(
                        modifier = Modifier
                            .size(82.dp)
                            .clip(CircleShape),
                        color = MaterialTheme.colorScheme.secondary,
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(
                                initials,
                                color = Color.White,
                                style = MaterialTheme.typography.displaySmall,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                    }

                    Spacer(Modifier.size(16.dp))

                    Text(
                        displayName,
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Bold,
                    )

                    Spacer(Modifier.size(4.dp))

                    Text(
                        npubLabel,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )

                    if (noteCount > 0) {
                        Spacer(Modifier.size(8.dp))
                        Text(
                            "$noteCount ${if (noteCount == 1) "post" else "posts"}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                HorizontalDivider()

                // Posts section: lazy-loaded timeline (D8: render verbatim from snapshot).
                if (cards.isEmpty()) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(16.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            "No posts yet",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                } else {
                    LazyColumn(Modifier.fillMaxSize()) {
                        itemsIndexed(
                            cards,
                            key = { _, root -> root.card.id },
                        ) { index, root ->
                            NoteRow(
                                root.card.id,
                                emptyMap(),
                                cardLookup,
                                model = model,
                            )
                            // Author display names resolve via LocalResolvedProfiles,
                            // provided by the enclosing CompositionLocalProvider.
                            if (index < cards.lastIndex) {
                                HorizontalDivider(Modifier.padding(start = 56.dp))
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun abbreviateMiddle(value: String, prefix: Int, suffix: Int): String {
    if (value.length <= prefix + suffix + 1) return value
    return "${value.take(prefix)}…${value.takeLast(suffix)}"
}
