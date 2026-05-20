package org.nmp.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Speed
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
import org.nmp.android.ui.DiagnosticsScreen
import org.nmp.android.ui.TimelineScreen

/**
 * Single-activity Compose host. Mirrors iOS Chirp `RootShell`'s tabs, but for L1
 * read-side parity there is no onboarding gate — see `android/README.md`
 * §Deviations. The kernel auto-loads the bootstrap-pubkey feed on `start()`.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                val model: KernelModel = viewModel()
                model.start()
                RootTabs(model)
            }
        }
    }
}

@Composable
private fun RootTabs(model: KernelModel) {
    var tab by remember { mutableIntStateOf(0) }
    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = tab == 0,
                    onClick = { tab = 0 },
                    icon = { Icon(Icons.Filled.Home, contentDescription = null) },
                    label = { Text("Timeline") },
                )
                NavigationBarItem(
                    selected = tab == 1,
                    onClick = { tab = 1 },
                    icon = { Icon(Icons.Filled.Speed, contentDescription = null) },
                    label = { Text("Diagnostics") },
                )
            }
        },
    ) { inner ->
        when (tab) {
            0 -> TimelineScreen(model, Modifier.padding(inner))
            else -> DiagnosticsScreen(model, Modifier.padding(inner))
        }
    }
}
