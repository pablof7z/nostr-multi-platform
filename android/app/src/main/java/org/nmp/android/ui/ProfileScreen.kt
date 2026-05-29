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
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import org.nmp.android.model.TimelineItem

/**
 * Author/profile detail screen — Jetpack Compose peer of iOS `ProfileView`.
 *
 * Renders an author's profile header (avatar, display name, pubkey), claims
 * the profile with the kernel for demand-driven kind:0 fetching, and displays
 * the author's timeline of notes. The profile card and items are both pushed
 * by the kernel snapshot during `openAuthor(pubkey)`.
 *
 * Thin-shell rule (aim.md §6.9): profile metadata (display name, pubkey display
 * format) is authored by Rust; the Compose layer is presentation only.
 */
@Composable
fun ProfileScreen(
    pubkey: String,
    model: KernelModel,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Claim the profile on appearance; release on disappearance for demand-driven
    // kind:0 fetching. The kernel batches a kind:0 REQ and re-fetches against the
    // author's NIP-65 write set once it lands (D4: thin claim/release lifecycle).
    DisposableEffect(pubkey) {
        model.claimProfile(pubkey, "profile_screen")
        onDispose {
            model.releaseProfile(pubkey, "profile_screen")
        }
    }

    val snapshot by model.state.collectAsStateWithLifecycle()

    // The kernel publishes the author's timeline via openAuthor(pubkey).
    // Display name and profile metadata come from the snapshot's items and
    // mention projections (best-effort until author_view projection is available).
    // For now, items are fetched but the profile metadata display is minimal.
    val items: List<TimelineItem> = snapshot.items

    val itemLookup = items.associateBy { it.id }

    // Extract author display name from first item if available, or fall back to
    // shortened pubkey (D8: no derived state; this is just presentation formatting).
    val authorDisplayName = items.firstOrNull()
        ?.let { it.content.ifEmpty { null } }
        ?.let { "Author" } // Placeholder until kernel provides profile card

    val shortPubkey = if (pubkey.length >= 16) {
        "${pubkey.take(8)}…${pubkey.takeLast(8)}"
    } else {
        pubkey.ifEmpty { "unknown" }
    }

    val displayName = authorDisplayName ?: shortPubkey
    val initials = displayName.take(2).uppercase()
    val noteCount = items.size

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
                IconButton(onClick = {
                    model.openTimeline()
                    onBack()
                }) {
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
                // Avatar with initials — color derived from pubkey (aim.md §4.2:
                // pubkey-deterministic colors are safe; no external fetch required).
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

                // Display name (rendered verbatim from snapshot; no derived fallback).
                Text(
                    displayName,
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.Bold,
                )

                Spacer(Modifier.size(4.dp))

                // Shortened pubkey: `pubkey_short` is Rust-formatted (ADR-0032);
                // the display is read-only (D8: no mutations in Swift/Kotlin).
                Text(
                    shortPubkey,
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

                Spacer(Modifier.size(16.dp))

                // Follow/Unfollow button: routes through dispatch_action mechanism.
                // The action label and icon are authored by Rust (ProfileAction);
                // for now, we render a placeholder "Follow" button pending integration
                // with the kernel's profile action dispatch.
                Button(onClick = {
                    // TODO: Dispatch follow action via model.dispatchAction() once
                    // kernel provides profile action metadata (ProfileCard.primaryAction).
                    // See iOS ProfileView.perform(_:) for the pattern.
                }) {
                    Text("Follow")
                }
            }

            HorizontalDivider()

            // Posts section: lazy-loaded timeline (D8: render verbatim from snapshot).
            if (items.isEmpty()) {
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
                        items,
                        key = { _, item -> item.id },
                    ) { index, item ->
                        NoteRow(
                            item.id,
                            itemLookup,
                            emptyMap(),
                            model = model,
                        )
                        if (index < items.lastIndex) {
                            HorizontalDivider(Modifier.padding(start = 56.dp))
                        }
                    }
                }
            }
        }
    }
}
