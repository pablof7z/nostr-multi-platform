package org.nmp.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountBalanceWallet
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.MailOutline
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Wifi
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
import org.nmp.android.ui.DmScreen
import org.nmp.android.ui.GroupsScreen
import org.nmp.android.ui.RelayScreen
import org.nmp.android.ui.SignInScreen
import org.nmp.android.ui.TimelineScreen
import org.nmp.android.ui.WalletScreen

/**
 * Single-activity Compose host. Mirrors iOS Chirp `RootShell`'s tabs, but for L1
 * read-side parity there is no onboarding gate yet. The Rust JNI bridge seeds
 * Chirp's shared default relays, and the Timeline tab explicitly opens the
 * Rust-owned timeline view.
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
                    icon = { Icon(Icons.Filled.MailOutline, contentDescription = null) },
                    label = { Text("DMs") },
                )
                NavigationBarItem(
                    selected = tab == 2,
                    onClick = { tab = 2 },
                    icon = { Icon(Icons.Filled.Group, contentDescription = null) },
                    label = { Text("Groups") },
                )
                NavigationBarItem(
                    selected = tab == 3,
                    onClick = { tab = 3 },
                    icon = { Icon(Icons.Filled.Wifi, contentDescription = null) },
                    label = { Text("Relays") },
                )
                NavigationBarItem(
                    selected = tab == 4,
                    onClick = { tab = 4 },
                    icon = { Icon(Icons.Filled.AccountCircle, contentDescription = null) },
                    label = { Text("Account") },
                )
                NavigationBarItem(
                    selected = tab == 5,
                    onClick = { tab = 5 },
                    icon = { Icon(Icons.Filled.AccountBalanceWallet, contentDescription = null) },
                    label = { Text("Wallet") },
                )
                NavigationBarItem(
                    selected = tab == 6,
                    onClick = { tab = 6 },
                    icon = { Icon(Icons.Filled.Speed, contentDescription = null) },
                    label = { Text("Diagnostics") },
                )
            }
        },
    ) { inner ->
        when (tab) {
            0 -> TimelineScreen(model, Modifier.padding(inner))
            1 -> DmScreen(model, Modifier.padding(inner))
            2 -> GroupsScreen(model, Modifier.padding(inner))
            3 -> RelayScreen(model, Modifier.padding(inner))
            4 -> SignInScreen(model, Modifier.padding(inner))
            5 -> WalletScreen(model, Modifier.padding(inner))
            else -> DiagnosticsScreen(model, Modifier.padding(inner))
        }
    }
}
