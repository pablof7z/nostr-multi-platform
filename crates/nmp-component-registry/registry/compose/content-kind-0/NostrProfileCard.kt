// Requires: compose-ui, compose-foundation, compose-material3,
// io.coil-kt:coil-compose (>= 2.x). Kotlin 1.9+.
//
// Compose typed renderer for kind:0 profile metadata embeds. The model is
// hydrated from Rust's ProfileProjection; this file only formats raw fields.

package nmp.content

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil.compose.SubcomposeAsyncImage

public data class NostrProfileCardModel(
    val pubkey: String,
    val displayName: String? = null,
    val pictureUrl: String? = null,
    val about: String? = null,
    val nip05: String? = null,
)

@Composable
public fun NostrProfileCard(
    model: NostrProfileCardModel,
    modifier: Modifier = Modifier,
    onTap: (() -> Unit)? = null,
) {
    val tapModifier = if (onTap != null) modifier.clickable { onTap() } else modifier
    Column(
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = tapModifier
            .fillMaxWidth()
            .semantics {
                contentDescription = "${displayLabel(model)}, profile"
            },
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            ProfileAvatar(model = model, size = 44.dp)
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = displayLabel(model),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                model.nip05?.trim()?.takeIf { it.isNotEmpty() }?.let { nip05 ->
                    Text(
                        text = nip05.removePrefix("_@"),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Text(
                text = "profile",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
        }

        Text(
            text = shortHex(model.pubkey),
            style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )

        model.about?.trim()?.takeIf { it.isNotEmpty() }?.let { about ->
            Text(
                text = about,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun ProfileAvatar(model: NostrProfileCardModel, size: Dp) {
    val url = model.pictureUrl
    if (url.isNullOrEmpty()) {
        ProfileAvatarFallback(identityKey = model.pubkey, size = size)
        return
    }
    SubcomposeAsyncImage(
        model = url,
        contentDescription = null,
        contentScale = ContentScale.Crop,
        loading = { ProfileAvatarFallback(identityKey = model.pubkey, size = size) },
        error = { ProfileAvatarFallback(identityKey = model.pubkey, size = size) },
        modifier = Modifier
            .size(size)
            .clip(CircleShape),
    )
}

@Composable
private fun ProfileAvatarFallback(identityKey: String, size: Dp) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier
            .size(size)
            .clip(CircleShape)
            .background(NostrIdenticon.colorForPubkey(identityKey)),
    ) {
        Text(
            text = NostrIdenticon.initialsForPubkey(identityKey),
            color = Color.White,
            fontWeight = FontWeight.SemiBold,
            fontSize = 14.sp,
        )
    }
}

private fun displayLabel(model: NostrProfileCardModel): String =
    model.displayName?.trim()?.takeIf { it.isNotEmpty() } ?: shortHex(model.pubkey)

private fun shortHex(value: String): String =
    if (value.length <= 16) value else "${value.take(8)}...${value.takeLast(8)}"
