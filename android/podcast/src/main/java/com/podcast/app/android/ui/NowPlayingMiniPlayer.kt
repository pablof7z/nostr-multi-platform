package com.podcast.app.android.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import com.podcast.app.android.model.EpisodeRowPayload
import kotlinx.coroutines.delay

/**
 * Persistent NowPlaying mini-player docked above the navigation bar.
 *
 * T-podcast-android-8: wired to the shared [MediaController] from
 * [PodcastPlaybackService] (hoisted to MainActivity — single controller
 * shared by EpisodeDetailScreen and this composable; no sync hazard).
 *
 * D6 honest states:
 *   - [episode] null  → hidden via [AnimatedVisibility] — never shown blank.
 *   - [isPlaying] reflects the ExoPlayer state via [Player.Listener].
 *   - Progress bar hidden when [durationMs] is 0 or unknown (D6 — never fake).
 *
 * Scope (MVP):
 *   - Artwork: waveform glyph placeholder (real artwork loading = next iteration).
 *   - Scrubber: non-interactive progress indicator (drag-to-seek = later).
 *   - Tapping the bar navigates to EpisodeDetail via [onTap].
 *   - No skip-forward / dismiss (chapters/sleep-timer are deferred).
 *
 * Doctrine:
 *   - D0: no podcast/audio/RSS nouns in nmp-core (this is host-side).
 *   - D5: zero business logic — state sourced from ExoPlayer + Rust snapshot.
 *   - D6: honest state only — hidden when nothing plays; no fake progress.
 *   - D8: verbatim mirror of [EpisodeRowPayload] — no derived state.
 */
@Composable
fun NowPlayingMiniPlayer(
    episode: EpisodeRowPayload?,
    controller: MediaController?,
    onTap: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Observe isPlaying + position from the shared controller.
    var isPlaying by remember { mutableStateOf(controller?.isPlaying ?: false) }
    var positionMs by remember { mutableFloatStateOf(controller?.currentPosition?.toFloat() ?: 0f) }
    var durationMs by remember { mutableFloatStateOf(controller?.duration?.takeIf { it > 0 }?.toFloat() ?: 0f) }

    // Sync isPlaying from the controller when it changes (e.g. after play/pause
    // initiated from EpisodeDetailScreen rather than the mini-player itself).
    DisposableEffect(controller) {
        val ctrl = controller ?: return@DisposableEffect onDispose {}
        // Initialise from current controller state so the UI is consistent
        // on first composition even before the listener fires.
        isPlaying = ctrl.isPlaying
        positionMs = ctrl.currentPosition.toFloat()
        durationMs = ctrl.duration.takeIf { it > 0 }?.toFloat() ?: 0f

        val listener = object : Player.Listener {
            override fun onIsPlayingChanged(playing: Boolean) {
                isPlaying = playing
                if (!playing) positionMs = ctrl.currentPosition.toFloat()
            }

            override fun onPlaybackStateChanged(playbackState: Int) {
                durationMs = ctrl.duration.takeIf { it > 0 }?.toFloat() ?: 0f
                positionMs = ctrl.currentPosition.toFloat()
            }
        }
        ctrl.addListener(listener)
        onDispose { ctrl.removeListener(listener) }
    }

    // Poll position every 500 ms while playing (lightweight — ExoPlayer is truth).
    LaunchedEffect(isPlaying, controller) {
        while (isPlaying && controller != null) {
            positionMs = controller.currentPosition.toFloat()
            durationMs = controller.duration.takeIf { it > 0 }?.toFloat() ?: durationMs
            delay(500L)
        }
    }

    AnimatedVisibility(
        visible = episode != null,
        enter = slideInVertically(initialOffsetY = { it }),
        exit = slideOutVertically(targetOffsetY = { it }),
        modifier = modifier,
    ) {
        if (episode == null) return@AnimatedVisibility
        MiniPlayerBar(
            episode = episode,
            isPlaying = isPlaying,
            positionMs = positionMs,
            durationMs = durationMs,
            onPlayPause = {
                val ctrl = controller ?: return@MiniPlayerBar
                if (ctrl.isPlaying) ctrl.pause() else ctrl.play()
            },
            onTap = onTap,
        )
    }
}

@Composable
private fun MiniPlayerBar(
    episode: EpisodeRowPayload,
    isPlaying: Boolean,
    positionMs: Float,
    durationMs: Float,
    onPlayPause: () -> Unit,
    onTap: () -> Unit,
) {
    Surface(
        tonalElevation = 4.dp,
        shadowElevation = 4.dp,
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Column(modifier = Modifier.fillMaxWidth()) {
            // Progress indicator — only shown when duration is known (D6 honest).
            if (durationMs > 0f) {
                LinearProgressIndicator(
                    progress = { (positionMs / durationMs).coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(),
                    color = MaterialTheme.colorScheme.primary,
                    trackColor = MaterialTheme.colorScheme.surfaceVariant,
                )
            }

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable(onClick = onTap)
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                // Artwork placeholder — waveform glyph (real async load = later).
                Box(
                    modifier = Modifier
                        .size(40.dp)
                        .clip(RoundedCornerShape(6.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    Surface(
                        color = MaterialTheme.colorScheme.secondaryContainer,
                        modifier = Modifier.size(40.dp),
                        shape = RoundedCornerShape(6.dp),
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                // Waveform analogue using a built-in icon (no extra deps).
                                imageVector = Icons.Filled.PlayArrow,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSecondaryContainer,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    }
                }

                // Title + podcast name — expand to fill, ellipsize.
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Text(
                        text = episode.title.ifEmpty { "Now Playing" },
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (episode.podcastTitle.isNotEmpty()) {
                        Text(
                            text = episode.podcastTitle,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }

                // Play / Pause — tightly scoped tap target.
                IconButton(onClick = onPlayPause) {
                    Icon(
                        imageVector = if (isPlaying) Icons.Filled.Pause else Icons.Filled.PlayArrow,
                        contentDescription = if (isPlaying) "Pause" else "Play",
                        tint = MaterialTheme.colorScheme.onSurface,
                    )
                }
            }
        }
    }
}
