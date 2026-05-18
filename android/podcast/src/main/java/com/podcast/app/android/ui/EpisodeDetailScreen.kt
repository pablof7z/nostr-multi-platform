package com.podcast.app.android.ui

import android.content.ComponentName
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayCircle
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import com.podcast.app.android.model.EpisodeRowPayload
import com.podcast.app.android.playback.PodcastPlaybackService
import kotlinx.coroutines.delay

/**
 * Episode detail screen with real ExoPlayer audio playback.
 *
 * T-podcast-android-7: wires the Play button to an [ExoPlayer] hosted in
 * [PodcastPlaybackService] (foreground MediaSessionService) via [MediaController].
 * Playback survives screen-off. MVP scope: play / pause / elapsed position display.
 *
 * D6 honest states:
 *   - [audioUrl] empty → Play button disabled + "No audio available" label.
 *   - Connecting to session → button disabled while controller is being obtained.
 *   - Playing → shows Pause button.
 *   - Paused / stopped → shows Play button.
 *
 * All state is sourced from ExoPlayer (D8 — no fabricated progress bars).
 * No business logic on the Kotlin side (D5).
 *
 * Doctrine compliance:
 *   - D0: no podcast/audio/RSS nouns in crates/nmp-core.
 *   - D5: no business logic — all state from Rust snapshot + ExoPlayer.
 *   - D6: honest state — disabled button with reason when no audio URL.
 *   - D8: verbatim mirror of EpisodeRowPayload — no derived state.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EpisodeDetailScreen(
    episode: EpisodeRowPayload,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val hasAudio = episode.audioUrl.isNotEmpty()

    // MediaController is nullable while the session connection is being established.
    var controller by remember { mutableStateOf<MediaController?>(null) }
    var controllerFuture by remember { mutableStateOf<ListenableFuture<MediaController>?>(null) }

    // Playback state observed from the controller.
    var isPlaying by remember { mutableStateOf(false) }
    var positionMs by remember { mutableLongStateOf(0L) }

    // Connect to PodcastPlaybackService via MediaController.
    DisposableEffect(Unit) {
        if (!hasAudio) return@DisposableEffect onDispose {}

        val token = SessionToken(
            context,
            ComponentName(context, PodcastPlaybackService::class.java),
        )
        val future = MediaController.Builder(context, token).buildAsync()
        controllerFuture = future

        future.addListener(
            {
                val ctrl = runCatching { future.get() }.getOrNull() ?: return@addListener
                controller = ctrl
                isPlaying = ctrl.isPlaying
            },
            { runnable -> runnable.run() }, // direct executor — already on main thread callback
        )

        onDispose {
            controller?.release()
            controller = null
            MediaController.releaseFuture(future)
            controllerFuture = null
        }
    }

    // Poll position every 500 ms while playing (lightweight — ExoPlayer is the truth).
    LaunchedEffect(isPlaying) {
        while (isPlaying) {
            positionMs = controller?.currentPosition ?: 0L
            delay(500L)
        }
    }

    // Keep isPlaying in sync via a Player.Listener.
    DisposableEffect(controller) {
        val ctrl = controller ?: return@DisposableEffect onDispose {}
        val listener = object : Player.Listener {
            override fun onIsPlayingChanged(playing: Boolean) {
                isPlaying = playing
                if (!playing) positionMs = ctrl.currentPosition
            }
        }
        ctrl.addListener(listener)
        onDispose { ctrl.removeListener(listener) }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = episode.podcastTitle.ifEmpty { "Episode" },
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back to episode list",
                        )
                    }
                },
            )
        },
    ) { inner ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(inner)
                .verticalScroll(rememberScrollState()),
        ) {
            // --- Hero section ---
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 20.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = episode.title,
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = episode.podcastTitle,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
                // Meta row: pub date + duration
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (episode.pubDateStr.isNotEmpty()) {
                        Text(
                            text = episode.pubDateStr,
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    if (episode.durationStr.isNotEmpty()) {
                        Text(
                            text = episode.durationStr,
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }

            // --- Play / Pause affordance ---
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (!hasAudio) {
                    // D6: honest disabled state — visible reason, never silent.
                    OutlinedButton(
                        onClick = {},
                        enabled = false,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Icon(
                            imageVector = Icons.Filled.PlayCircle,
                            contentDescription = null,
                            modifier = Modifier
                                .size(20.dp)
                                .padding(end = 4.dp),
                        )
                        Text(text = "No audio available")
                    }
                } else {
                    FilledTonalButton(
                        onClick = {
                            val ctrl = controller ?: return@FilledTonalButton
                            if (ctrl.isPlaying) {
                                ctrl.pause()
                            } else {
                                // If controller isn't already on this episode, set the item.
                                val currentUri = ctrl.currentMediaItem?.localConfiguration?.uri?.toString()
                                if (currentUri != episode.audioUrl) {
                                    ctrl.setMediaItem(MediaItem.fromUri(episode.audioUrl))
                                    ctrl.prepare()
                                }
                                ctrl.play()
                            }
                        },
                        enabled = controller != null,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Icon(
                            imageVector = if (isPlaying) Icons.Filled.Pause else Icons.Filled.PlayCircle,
                            contentDescription = null,
                            modifier = Modifier
                                .size(20.dp)
                                .padding(end = 4.dp),
                        )
                        Text(text = if (isPlaying) "Pause" else "Play Episode")
                    }

                    // Elapsed position — only shown when position is meaningful.
                    if (positionMs > 0L) {
                        Text(
                            text = formatPositionMs(positionMs),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.align(Alignment.CenterHorizontally),
                        )
                    }
                }
            }

            // --- Show notes / description ---
            if (!episode.summary.isNullOrEmpty()) {
                HorizontalDivider(
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 16.dp),
                    thickness = 0.5.dp,
                    color = MaterialTheme.colorScheme.outlineVariant,
                )
                Column(
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 0.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        text = "Show Notes",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        text = episode.summary,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            // Bottom padding so content clears the navigation bar.
            Box(modifier = Modifier.padding(bottom = 24.dp))
        }
    }
}

/** Format elapsed ms as "m:ss" or "h:mm:ss". */
private fun formatPositionMs(ms: Long): String {
    val totalSec = ms / 1000L
    val h = totalSec / 3600L
    val m = (totalSec % 3600L) / 60L
    val s = totalSec % 60L
    return if (h > 0L) "$h:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}"
    else "$m:${s.toString().padStart(2, '0')}"
}
