// Requires: compose-ui, compose-foundation, compose-material3,
// io.coil-kt:coil-compose (>= 2.x). Kotlin 1.9+.
//
// Compose typed renderer for kind:30023 long-form articles (NIP-23) embedded
// inline inside surrounding note text. Mirrors the SwiftUI `ArticleEmbed`
// (Registry/ArticleEmbed.swift) and the TUI article renderer, so every surface
// paints the same medium-like card instead of falling back to the generic
// quote card. Renders:
//   • optional hero image (full-width, 16:9 crop)
//   • article title (large, semibold)
//   • optional summary line
//   • author byline: avatar + display name + "article · kind:30023"
//
// The app hydrates `NostrArticleCardModel` from a resolved `claimed_events`
// entry (title/summary/image come from the event's NIP-23 tags); the card only
// renders. Depends on `compose/content-core`.

package org.nmp.gallery.registry

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
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

public data class NostrArticleCardModel(
    val id: String,
    val authorPubkey: String? = null,
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val title: String? = null,
    val summary: String? = null,
    val heroImageUrl: String? = null,
)

@Composable
public fun NostrArticleCard(
    model: NostrArticleCardModel,
    modifier: Modifier = Modifier,
    onTap: (() -> Unit)? = null,
) {
    val renderer = LocalNostrContentRenderer.current
    val tap = onTap ?: { renderer.callbacks.onEventRefTap(model.id) }
    Column(
        verticalArrangement = Arrangement.spacedBy(10.dp),
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .clickable { tap() }
            .semantics { contentDescription = "Open article" },
    ) {
        val hero = model.heroImageUrl
        if (!hero.isNullOrEmpty()) {
            SubcomposeAsyncImage(
                model = hero,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                loading = { HeroPlaceholder() },
                error = { HeroPlaceholder() },
                modifier = Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f)
                    .clip(RoundedCornerShape(8.dp)),
            )
        }
        val title = model.title?.trim()
        if (!title.isNullOrEmpty()) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                color = renderer.textColor,
            )
        }
        val summary = model.summary?.trim()
        if (!summary.isNullOrEmpty()) {
            Text(
                text = summary,
                style = MaterialTheme.typography.bodyMedium,
                color = renderer.secondaryTextColor,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            ArticleAvatar(model = model, size = 24.dp)
            Text(
                text = bylineLabel(model),
                style = MaterialTheme.typography.bodySmall,
                fontWeight = FontWeight.Medium,
                color = renderer.secondaryTextColor,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(
                text = "article · kind:30023",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                color = renderer.secondaryTextColor.copy(alpha = 0.7f),
            )
        }
    }
}

@Composable
private fun HeroPlaceholder() {
    val renderer = LocalNostrContentRenderer.current
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(16f / 9f)
            .clip(RoundedCornerShape(8.dp))
            .background(renderer.codeBackgroundColor),
    )
}

@Composable
private fun ArticleAvatar(model: NostrArticleCardModel, size: Dp) {
    val identityKey = model.authorPubkey ?: model.id
    val url = model.authorPictureUrl
    if (url.isNullOrEmpty()) {
        ArticleAvatarFallback(identityKey = identityKey, size = size)
        return
    }
    SubcomposeAsyncImage(
        model = url,
        contentDescription = null,
        contentScale = ContentScale.Crop,
        loading = { ArticleAvatarFallback(identityKey = identityKey, size = size) },
        error = { ArticleAvatarFallback(identityKey = identityKey, size = size) },
        modifier = Modifier
            .size(size)
            .clip(CircleShape),
    )
}

@Composable
private fun ArticleAvatarFallback(identityKey: String, size: Dp) {
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
            fontSize = 11.sp,
        )
    }
}

private fun bylineLabel(model: NostrArticleCardModel): String {
    val display = model.authorDisplayName
    if (!display.isNullOrEmpty()) return display
    val pubkey = model.authorPubkey
    if (!pubkey.isNullOrEmpty()) {
        return if (pubkey.length <= 12) pubkey else "${pubkey.take(8)}…${pubkey.takeLast(4)}"
    }
    return "article"
}
