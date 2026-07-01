package org.nmp.registry

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil.compose.SubcomposeAsyncImage
import nmp.content.NostrIdenticon
import nmp.content.NostrIdenticonGrid
import java.util.UUID

/**
 * Circular avatar for a Nostr pubkey. Shows the profile picture when the
 * host projection has it; falls back to a deterministic identicon derived
 * from `pubkey`.
 *
 * Replace [SubcomposeAsyncImage] with Glide/Picasso/custom if you already
 * have an image loader — the identicon fallback is self-contained.
 *
 * Depends on `compose/user-avatar` for [ProfileWire] / [NostrProfileHost]
 * and `compose/content-core` for [NostrIdenticon].
 */
@Composable
fun NostrAvatar(
    pubkey: String,
    avatarUrl: String? = null,
    size: Dp = 40.dp,
    modifier: Modifier = Modifier,
    consumerId: String? = null,
) {
    val profileHost = LocalNostrProfileHost.current
    val resolvedConsumerId = remember(pubkey, consumerId) {
        consumerId ?: "nostr-avatar.${UUID.randomUUID()}"
    }
    val resolvedAvatarUrl = avatarUrl ?: profileHost?.profileForPubkey(pubkey)?.avatarUrl

    DisposableEffect(pubkey, resolvedConsumerId) {
        profileHost?.resolveProfileRef(pubkey, resolvedConsumerId)
        onDispose {
            profileHost?.releaseProfileRef(pubkey, resolvedConsumerId)
        }
    }

    val baseModifier = modifier
        .size(size)
        .clip(CircleShape)
        .clearAndSetSemantics {}

    if (!resolvedAvatarUrl.isNullOrEmpty()) {
        SubcomposeAsyncImage(
            model = resolvedAvatarUrl,
            contentDescription = null,
            modifier = baseModifier,
            error = { NostrIdenticonGrid(pubkey = pubkey, size = size) },
            loading = { NostrIdenticonGrid(pubkey = pubkey, size = size) },
        )
    } else {
        NostrIdenticonGrid(pubkey = pubkey, size = size, modifier = baseModifier)
    }
}

/** Convenience overload accepting a [ProfileWire]. */
@Composable
fun NostrAvatar(
    profile: ProfileWire,
    size: Dp = 40.dp,
    modifier: Modifier = Modifier,
) = NostrAvatar(
    pubkey = profile.pubkey,
    avatarUrl = profile.avatarUrl,
    size = size,
    modifier = modifier,
)
