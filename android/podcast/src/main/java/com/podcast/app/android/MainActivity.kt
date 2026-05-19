package com.podcast.app.android

import android.content.ComponentName
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Headphones
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import com.podcast.app.android.model.EpisodeRowPayload
import com.podcast.app.android.playback.PodcastPlaybackService
import com.podcast.app.android.ui.EpisodeDetailScreen
import com.podcast.app.android.ui.EpisodeListScreen
import com.podcast.app.android.ui.LibraryScreen
import com.podcast.app.android.ui.NowPlayingMiniPlayer

/**
 * NmpPodcast single-activity Compose host.
 *
 * T-podcast-android-3: adds episode-list navigation.
 * T-podcast-android-5: adds episode-detail navigation.
 * T-podcast-android-8: hoists [MediaController] to this level so the
 *   [NowPlayingMiniPlayer] and [EpisodeDetailScreen] share a single session
 *   connection — no sync hazard from two independent controllers.
 *
 * Doctrine: Kotlin shell is parity-only. No business logic, no derived state
 * (D5 / D8). The kernel snapshot drives every UI mutation.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                val model: PodcastKernelModel = viewModel()
                model.start()
                RootTabs(model)
            }
        }
    }
}

@Composable
private fun RootTabs(model: PodcastKernelModel) {
    val context = LocalContext.current

    var tab by remember { mutableIntStateOf(0) }
    // Navigation state for the Library tab.
    var selectedPodcastId by remember { mutableStateOf<String?>(null) }
    var selectedEpisode by remember { mutableStateOf<EpisodeRowPayload?>(null) }

    // T-podcast-android-8: hoist MediaController to this level so it is shared
    // between EpisodeDetailScreen and NowPlayingMiniPlayer. One controller =
    // one session connection, no state divergence.
    var controller by remember { mutableStateOf<MediaController?>(null) }
    var controllerFuture by remember { mutableStateOf<ListenableFuture<MediaController>?>(null) }

    DisposableEffect(Unit) {
        val token = SessionToken(
            context,
            ComponentName(context, PodcastPlaybackService::class.java),
        )
        val future = MediaController.Builder(context, token).buildAsync()
        controllerFuture = future
        future.addListener(
            {
                controller = runCatching { future.get() }.getOrNull()
            },
            { runnable -> runnable.run() },
        )
        onDispose {
            controller?.release()
            controller = null
            MediaController.releaseFuture(future)
            controllerFuture = null
        }
    }

    // Observe nowPlaying from the model to drive the mini-player.
    val nowPlayingEpisode by model.nowPlaying.collectAsState()

    Scaffold(
        bottomBar = {
            Column {
                // NowPlaying mini-player sits between content and the nav bar.
                NowPlayingMiniPlayer(
                    episode = nowPlayingEpisode,
                    controller = controller,
                    onTap = {
                        // Tap navigates to EpisodeDetail for the playing episode.
                        val ep = nowPlayingEpisode ?: return@NowPlayingMiniPlayer
                        val pid = ep.podcastId.takeIf { it.isNotEmpty() } ?: return@NowPlayingMiniPlayer
                        tab = 0
                        selectedPodcastId = pid
                        selectedEpisode = ep
                    },
                    // D6: clear nowPlaying when ExoPlayer reports STATE_ENDED or
                    // STATE_IDLE so the mini-player hides instead of staying
                    // frozen on the last-played episode (honest state — no fake UI).
                    onPlaybackEnded = { model.setNowPlaying(null) },
                )
                NavigationBar {
                    NavigationBarItem(
                        selected = tab == 0,
                        onClick = {
                            tab = 0
                            if (selectedEpisode != null) {
                                selectedEpisode = null
                            } else if (selectedPodcastId != null) {
                                selectedPodcastId = null
                                model.onBackFromEpisodes()
                            }
                        },
                        icon = { Icon(Icons.Filled.Headphones, contentDescription = null) },
                        label = { Text("Library") },
                    )
                }
            }
        },
    ) { inner ->
        when {
            // Episode detail — deepest level
            tab == 0 && selectedPodcastId != null && selectedEpisode != null -> {
                EpisodeDetailScreen(
                    episode = selectedEpisode!!,
                    controller = controller,
                    onPlay = { ep -> model.setNowPlaying(ep) },
                    onBack = { selectedEpisode = null },
                    modifier = Modifier.padding(inner),
                )
            }
            // Episode list — second level
            tab == 0 && selectedPodcastId != null -> {
                EpisodeListScreen(
                    podcastId = selectedPodcastId!!,
                    model = model,
                    onBack = {
                        selectedPodcastId = null
                        model.onBackFromEpisodes()
                    },
                    onEpisodeSelected = { episode ->
                        selectedEpisode = episode
                    },
                    modifier = Modifier.padding(inner),
                )
            }
            // Library — top level
            else -> {
                LibraryScreen(
                    model = model,
                    onPodcastSelected = { podcastId ->
                        selectedPodcastId = podcastId
                    },
                    modifier = Modifier.padding(inner),
                )
            }
        }
    }
}
