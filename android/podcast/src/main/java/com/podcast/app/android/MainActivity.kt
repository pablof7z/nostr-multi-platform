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
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import com.podcast.app.android.ui.LibraryScreen

/**
 * NmpPodcast single-activity Compose host (T157 step 1).
 *
 * iOS parity: this mirrors `ios/NmpPodcast/.../ContentView.swift`'s `TabView`
 * structure. The Swift app has four tabs (Feed, Ask, Insights, Library); we
 * ship only **Library** in step 1 because that's the first tab `podcast-core`
 * will be able to populate once the kernel boundary lands
 * (see `docs/perf/m11/T-podcast-gap-1.md`). Additional tabs are wired in
 * subsequent T157-N iterations.
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
    // Step 1 ships only Library. Tab state is kept so subsequent iterations
    // can drop in Feed / Ask / Insights without restructuring the chrome.
    var tab by remember { mutableIntStateOf(0) }
    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = tab == 0,
                    onClick = { tab = 0 },
                    icon = { Icon(Icons.Filled.Headphones, contentDescription = null) },
                    label = { Text("Library") },
                )
            }
        },
    ) { inner ->
        when (tab) {
            0 -> LibraryScreen(model, Modifier.padding(inner))
            else -> LibraryScreen(model, Modifier.padding(inner))
        }
    }
}
