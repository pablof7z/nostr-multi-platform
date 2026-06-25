package org.nmp.android.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.widget.MediaController
import android.widget.VideoView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

@Composable
internal fun RemoteImage(url: String, modifier: Modifier = Modifier) {
    var bitmap by remember(url) { mutableStateOf<Bitmap?>(null) }
    var failed by remember(url) { mutableStateOf(false) }
    LaunchedEffect(url) {
        failed = false
        bitmap = withContext(Dispatchers.IO) { loadBitmap(url) }
        failed = bitmap == null
    }

    when {
        bitmap != null -> Image(
            bitmap = bitmap!!.asImageBitmap(),
            contentDescription = null,
            modifier = modifier
                .fillMaxWidth()
                .heightIn(max = 360.dp)
                .clip(RoundedCornerShape(8.dp)),
            contentScale = ContentScale.FillWidth,
        )
        failed -> MediaFallback("Image failed to load")
        else -> MediaFallback("Loading image")
    }
}

@Composable
internal fun RemoteVideo(url: String, modifier: Modifier = Modifier) {
    AndroidView(
        modifier = modifier
            .fillMaxWidth()
            .height(220.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        factory = { ctx ->
            VideoView(ctx).apply {
                val controls = MediaController(ctx)
                controls.setAnchorView(this)
                setMediaController(controls)
                setVideoURI(Uri.parse(url))
                setOnPreparedListener { player ->
                    player.isLooping = false
                    seekTo(1)
                }
            }
        },
        update = { view ->
            if (view.tag != url) {
                view.tag = url
                view.stopPlayback()
                view.setVideoURI(Uri.parse(url))
                view.seekTo(1)
            }
        },
    )
}

@Composable
internal fun RemoteAudio(url: String, modifier: Modifier = Modifier) {
    AndroidView(
        modifier = modifier
            .fillMaxWidth()
            .height(72.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        factory = { ctx ->
            VideoView(ctx).apply {
                val controls = MediaController(ctx)
                controls.setAnchorView(this)
                setMediaController(controls)
                setVideoURI(Uri.parse(url))
            }
        },
        update = { view ->
            if (view.tag != url) {
                view.tag = url
                view.stopPlayback()
                view.setVideoURI(Uri.parse(url))
            }
        },
    )
}

@Composable
private fun MediaFallback(label: String) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(120.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(12.dp),
        )
    }
}

private fun loadBitmap(url: String): Bitmap? = runCatching {
    val connection = URL(url).openConnection() as HttpURLConnection
    connection.connectTimeout = 8_000
    connection.readTimeout = 12_000
    connection.instanceFollowRedirects = true
    connection.inputStream.use { BitmapFactory.decodeStream(it) }
}.getOrNull()
