package org.nmp.android.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil.compose.SubcomposeAsyncImage
import java.util.UUID

/**
 * Circular avatar for a Nostr pubkey. Shows the profile picture when the
 * host projection has it; falls back to a deterministic identicon derived
 * from `pubkey`.
 *
 * Replace [SubcomposeAsyncImage] with Glide/Picasso/custom if you already
 * have an image loader — the identicon fallback is self-contained.
 *
 * Depends on `compose/user-avatar` for [ProfileWire] and [NostrProfileHost].
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

// ── Identicon ────────────────────────────────────────────────────────────────

/**
 * 5×5 symmetric pixel-grid identicon, GitHub-style. Deterministic from the
 * pubkey via djb2: the lower 15 bits of the hash decide which of the 15
 * left-half cells (3 columns × 5 rows) are filled; columns 3–4 mirror
 * columns 1–0 so the result is horizontally symmetric. Color is derived
 * from djb2 % 360 as the HSV hue with S=0.55, V=0.75.
 *
 * Algorithm is byte-identical to the Swift implementation in
 * `swiftui/user-avatar/NostrAvatar.swift` and
 * `swiftui/content-core/ContentTreeWire.swift`. Same pubkey → same grid and
 * color on every platform.
 */
internal object NostrIdenticon {
    fun colorForPubkey(pubkey: String): Color {
        val hue = (djb2(pubkey) % 360u).toFloat()
        return Color.hsv(hue, 0.55f, 0.75f)
    }

    /** Returns 5 rows of 5 booleans: true = filled cell, false = empty. */
    fun cellsForPubkey(pubkey: String): Array<BooleanArray> {
        val hash = djb2(pubkey)
        return Array(5) { row ->
            BooleanArray(5) { col ->
                val mirrorCol = if (col < 3) col else 4 - col
                val bit = row * 3 + mirrorCol
                (hash shr bit) and 1u == 1u
            }
        }
    }

    private fun djb2(value: String): UInt {
        var hash: UInt = 5381u
        for (byte in value.encodeToByteArray()) {
            hash = hash * 33u + byte.toUByte().toUInt()
        }
        return hash
    }
}

/**
 * Renders the 5×5 symmetric identicon grid for [pubkey] using [NostrIdenticon].
 * Filled cells are drawn in the pubkey color; empty cells show the same color
 * at 15% opacity so the tile reads as a tinted patch rather than blank.
 *
 * Callers that need a circular avatar should apply `.clip(CircleShape)`;
 * [NostrAvatar] does this automatically via the base modifier.
 */
@Composable
internal fun NostrIdenticonGrid(pubkey: String, size: Dp, modifier: Modifier = Modifier) {
    val color = NostrIdenticon.colorForPubkey(pubkey)
    val cells = remember(pubkey) { NostrIdenticon.cellsForPubkey(pubkey) }
    Canvas(
        modifier = modifier
            .size(size)
            .background(color.copy(alpha = 0.15f)),
    ) {
        val spacing = 1f
        val cellPx = (minOf(this.size.width, this.size.height) - spacing * 4f) / 5f
        for (row in 0..4) {
            for (col in 0..4) {
                if (cells[row][col]) {
                    drawRect(
                        color = color,
                        topLeft = Offset(col * (cellPx + spacing), row * (cellPx + spacing)),
                        size = Size(cellPx, cellPx),
                    )
                }
            }
        }
    }
}
