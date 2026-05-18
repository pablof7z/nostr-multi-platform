package com.podcast.app.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import com.podcast.app.android.model.EpisodeRowPayload
import com.podcast.app.android.ui.EpisodeDetailScreen
import com.podcast.app.android.ui.EpisodeListScreen
import com.podcast.app.android.ui.LibraryScreen

/**
 * NmpPodcast single-activity Compose host.
 *
 * T-podcast-android-3: adds episode-list navigation. Tapping a podcast row
 * in [LibraryScreen] navigates to [EpisodeListScreen] for that podcast.
 *
 * T-podcast-android-5: adds episode-detail navigation. Tapping an episode row
 * in [EpisodeListScreen] navigates to [EpisodeDetailScreen]. Navigation is a
 * three-level stack managed without a full NavController — two remember states
 * suffice for the Library → EpisodeList → EpisodeDetail hierarchy.
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
    var tab by remember { mutableIntStateOf(0) }
    // Navigation state for the Library tab.
    // null = library, non-null = episode list for that podcast.
    var selectedPodcastId by remember { mutableStateOf<String?>(null) }
    // null = episode list, non-null = episode detail for that episode.
    var selectedEpisode by remember { mutableStateOf<EpisodeRowPayload?>(null) }

    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = tab == 0,
                    onClick = {
                        tab = 0
                        // Tapping Library tab collapses to top level.
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
        },
    ) { inner ->
        when {
            // Episode detail — deepest level
            tab == 0 && selectedPodcastId != null && selectedEpisode != null -> {
                EpisodeDetailScreen(
                    episode = selectedEpisode!!,
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
