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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.RefLiveness
import org.nmp.android.RefShape
import org.nmp.android.follow
import org.nmp.android.unfollow
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.ui.embed.EventClaimer
import org.nmp.android.ui.embed.LocalRefEventEnvelopes
import org.nmp.android.ui.embed.LocalEventClaimer
import org.nmp.android.components.NostrAvatar
import org.nmp.android.components.NostrNip05Badge
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
    // ADR-0063 Lane G (#1671): the open profile screen resolves the FULL
    // ProfileCard shape with Live liveness (keeps a tailing kind:0 sub while the
    // screen holds the key), via the unified resolve_ref seam.
    DisposableEffect(pubkey) {
        model.openAuthor(pubkey)
        model.resolveProfile(pubkey, profileConsumerId, RefShape.ProfileCard, RefLiveness.Live)
        onDispose {
            model.closeAuthor(pubkey)
            model.releaseProfile(pubkey, profileConsumerId)
        }
    }

    val snapshot by model.state.collectAsStateWithLifecycle()

    val projections = snapshot.projections
    val cards = projections?.flatFeeds?.get("nmp.feed.author.$pubkey")?.cards ?: emptyList()
    val cardLookup = cards.associate { it.card.id to it.card }

    // ADR-0063 Lane G: read the open-profile card per-key from the `refs.profile`
    // keyed-ref cache (the source of truth, D4 — no app-side profile cache).
    // Re-read whenever THIS pubkey's row commits (per-key reactivity), never via
    // a whole-map snapshot scan.
    var profileRowVersion by remember(pubkey) { mutableStateOf(0) }
    LaunchedEffect(pubkey) {
        model.profileRowChanged.collect { change ->
            if (change.rowKey == pubkey) profileRowVersion++
        }
    }
    val profileCard: ProfileCard? = remember(pubkey, profileRowVersion) {
        model.profileCard(pubkey)
    }

    val shortPubkey = abbreviateMiddle(pubkey.ifEmpty { "unknown" }, prefix = 8, suffix = 8)
    // ADR-0032 / V-115: `npub` is no longer sent by the projection (always
    // ""). Derive the display identifier on the host side via the kernel's
    // cached NIP-19 encoder (`nmp_app_encode_profile`). Falls back to the
    // short-hex abbreviation when the kernel is unavailable or the pubkey is
    // invalid — matches iOS ProfileView behaviour.
    val npubLabel = model.encodeProfile(pubkey)
        ?.takeIf { it.isNotEmpty() }
        ?.let { abbreviateMiddle(it, prefix = 10, suffix = 6) }
        ?: shortPubkey

    val displayName = profileCard?.displayName?.takeIf { it.isNotEmpty() } ?: shortPubkey
    val nip05 = profileCard?.nip05?.takeIf { it.isNotEmpty() }
    val noteCount = cards.size
    val following = projections?.followList?.follows?.contains(pubkey) == true
    val profileHost = rememberKernelProfileHost(model)
    val eventClaimer: EventClaimer = { uri, consumerId, claim ->
        if (claim) model.claimEvent(uri, consumerId)
        else model.releaseEvent(uri, consumerId)
    }
    val refEventEnvelopes = projections?.refEventEnvelopes ?: emptyMap()

    CompositionLocalProvider(
        LocalEventClaimer provides eventClaimer,
        LocalRefEventEnvelopes provides refEventEnvelopes,
        LocalNostrProfileHost provides profileHost,
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
                    NostrAvatar(
                        pubkey = pubkey,
                        size = 82.dp,
                        consumerId = "profile_header-$pubkey",
                    )

                    Spacer(Modifier.size(16.dp))

                    Text(
                        displayName,
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Bold,
                    )

                    if (nip05 != null) {
                        Spacer(Modifier.size(4.dp))
                        NostrNip05Badge(nip05 = nip05)
                    }

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

                    Spacer(Modifier.size(12.dp))
                    FollowButton(
                        pubkey = pubkey,
                        model = model,
                        following = following,
                        activePubkey = snapshot.activeAccount,
                    )
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
                            // Author display names resolve per-key via the
                            // refs.profile keyed-ref cache through LocalNostrProfileHost,
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

/**
 * Follow / Unfollow action. The label reads the Rust-owned `nmp.follow_list`
 * projection; this shell only dispatches actions and owns no follow state.
 */
@Composable
private fun FollowButton(
    pubkey: String,
    model: KernelModel,
    following: Boolean,
    activePubkey: String?,
) {
    if (pubkey.isEmpty() || pubkey == activePubkey) return
    if (following) {
        OutlinedButton(onClick = { model.unfollow(pubkey) }) {
            Text("Following")
        }
    } else {
        Button(
            onClick = { model.follow(pubkey) },
            colors = ButtonDefaults.buttonColors(),
        ) {
            Text("Follow")
        }
    }
}

private fun abbreviateMiddle(value: String, prefix: Int, suffix: Int): String {
    if (value.length <= prefix + suffix + 1) return value
    return "${value.take(prefix)}…${value.takeLast(suffix)}"
}
