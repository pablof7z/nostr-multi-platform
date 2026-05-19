package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage

/**
 * Extensibility seam for media rendering — Compose mirror of the Swift
 * `NmpMediaRenderer` / `Environment(\.nmpMediaRenderer)` pair. Apps can
 * provide custom image / video renderers via [LocalNmpMediaRenderer] in a
 * `CompositionLocalProvider`.
 */
data class NmpMediaRenderer(
    val imageView: @Composable (url: String) -> Unit,
    val videoView: @Composable (url: String) -> Unit,
) {
    companion object {
        val Default = NmpMediaRenderer(
            imageView = { url -> DefaultImageView(url) },
            videoView = { url -> DefaultVideoView(url) },
        )
    }
}

val LocalNmpMediaRenderer = compositionLocalOf { NmpMediaRenderer.Default }

@Composable
private fun DefaultImageView(url: String) {
    AsyncImage(
        model = url,
        contentDescription = null,
        contentScale = ContentScale.Fit,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(max = 400.dp)
            .clip(RoundedCornerShape(10.dp)),
    )
}

@Composable
private fun DefaultVideoView(url: String) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(Color.Black.copy(alpha = 0.72f))
            .padding(12.dp),
    ) {
        Icon(
            imageVector = Icons.Filled.PlayArrow,
            contentDescription = null,
            tint = Color.White,
        )
        Column {
            Text(
                "Video",
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
            )
            Text(
                url.substringAfterLast('/'),
                color = Color.White.copy(alpha = 0.7f),
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Box(Modifier.fillMaxWidth())
    }
}
