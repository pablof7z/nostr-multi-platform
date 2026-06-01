package org.nmp.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotPendingWelcome
import org.nmp.android.model.MarmotSnapshot

/**
 * Marmot (MLS-over-Nostr encrypted groups) screen — Android peer of the iOS
 * `MarmotGroupsView` / `MarmotStore` surface. Minimal V-109 skeleton: list
 * groups, create a group, accept/decline invites, publish the key package, and
 * send a message inside a group.
 *
 * Thin-shell rule (aim.md §2): ZERO protocol logic here. State is read from the
 * `nmp.marmot.snapshot` / `nmp.marmot.messages` push projections; every write
 * is a fire-and-forget [KernelModel] call that routes through the Rust
 * `dispatch_action("nmp.marmot", …)` seam. Display strings (`displayName`,
 * `keyPackage.subtitle`, `invitesChipLabel`, `keyPackage.actionLabel`) are
 * Rust-owned and rendered verbatim.
 *
 * Marmot registration needs the active local signing key, so it is wired
 * reactively: whenever the active account changes, [KernelModel.registerMarmotIfNeeded]
 * is invoked with the app-support dir. A bunker/NIP-46 account has no local key,
 * so `isRegistered` stays false and the empty state explains the requirement.
 */
@Composable
fun GroupsScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val s by model.state.collectAsStateWithLifecycle()
    val activeAccount = s.activeAccount

    // Register the Marmot MLS identity once a local account is active. The dir
    // is the canonical Android app-support location; the MLS SQLite state lives
    // under it (`<dir>/marmot-mls-state.sqlite`).
    LaunchedEffect(activeAccount) {
        if (activeAccount.isNotEmpty()) {
            model.registerMarmotIfNeeded(context.filesDir.path)
        }
    }

    val snapshot = s.projections?.marmotSnapshot ?: MarmotSnapshot()
    val messagesByGroup = s.projections?.marmotMessages ?: emptyMap()

    var selectedGroupId by remember { mutableStateOf<String?>(null) }

    Box(modifier.fillMaxSize()) {
        val selected = selectedGroupId
        if (selected != null) {
            val group = snapshot.groups.firstOrNull { it.idHex == selected }
            if (group == null) {
                // Group disappeared (left/removed) — pop back to the list.
                selectedGroupId = null
            } else {
                GroupChatView(
                    model = model,
                    group = group,
                    messages = messagesByGroup[selected] ?: emptyList(),
                    onBack = { selectedGroupId = null },
                )
            }
        } else {
            GroupListScreen(
                model = model,
                snapshot = snapshot,
                onSelectGroup = { selectedGroupId = it },
            )
        }
    }
}

@Composable
private fun GroupListScreen(
    model: KernelModel,
    snapshot: MarmotSnapshot,
    onSelectGroup: (String) -> Unit,
) {
    var showCreate by remember { mutableStateOf(false) }

    Scaffold(
        floatingActionButton = {
            if (snapshot.isRegistered) {
                FloatingActionButton(onClick = { showCreate = true }) {
                    Icon(Icons.Filled.Add, contentDescription = "New group")
                }
            }
        },
    ) { inner ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(inner),
        ) {
            Row(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Groups", style = MaterialTheme.typography.headlineSmall)
                snapshot.invitesChipLabel?.let { label ->
                    Surface(
                        color = MaterialTheme.colorScheme.primaryContainer,
                        shape = RoundedCornerShape(12.dp),
                    ) {
                        Text(
                            label,
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
                        )
                    }
                }
            }
            HorizontalDivider()

            if (!snapshot.isRegistered) {
                NotRegisteredState(subtitle = snapshot.keyPackage.subtitle)
                return@Column
            }

            // V-62 diagnostic: MLS secrets live in process memory only.
            if (snapshot.keyringUnavailable) {
                WarningBanner(
                    "Keyring unavailable — group secrets are kept in memory only and " +
                        "will be lost on next launch.",
                )
            }
            // V-61 diagnostic: local MLS state may have diverged from the relay.
            if (snapshot.orphanedCommitCount > 0) {
                WarningBanner(
                    "A group commit may not have reached the relay. Sending is blocked " +
                        "until the group recovers.",
                )
            }

            LazyColumn(Modifier.fillMaxSize()) {
                item { KeyPackageRow(model, snapshot) }

                if (snapshot.pendingWelcomes.isNotEmpty()) {
                    item {
                        SectionHeader("Invites")
                    }
                    items(snapshot.pendingWelcomes, key = { it.idHex }) { welcome ->
                        PendingWelcomeRow(model, welcome)
                        HorizontalDivider()
                    }
                }

                item { SectionHeader("Your groups") }
                if (snapshot.groups.isEmpty()) {
                    item { EmptyGroupsHint() }
                } else {
                    items(snapshot.groups, key = { it.idHex }) { group ->
                        GroupRow(group = group, onClick = { onSelectGroup(group.idHex) })
                        HorizontalDivider()
                    }
                }
            }
        }
    }

    if (showCreate) {
        CreateGroupDialog(
            onDismiss = { showCreate = false },
            onCreate = { name, invitees ->
                model.createGroup(name = name, description = "", inviteeText = invitees)
                showCreate = false
            },
        )
    }
}

@Composable
private fun KeyPackageRow(model: KernelModel, snapshot: MarmotSnapshot) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text("Key package", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
            // Rust-owned subtitle, rendered verbatim (aim.md §6 AP1).
            Text(
                snapshot.keyPackage.subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.size(8.dp))
        OutlinedButton(onClick = { model.publishKeyPackage() }) {
            // Rust picks the verb ("Publish key package" / "Rotate key package").
            Text(snapshot.keyPackage.actionLabel.ifEmpty { "Publish" })
        }
    }
    HorizontalDivider()
}

@Composable
private fun PendingWelcomeRow(model: KernelModel, welcome: MarmotPendingWelcome) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(welcome.displayName, style = MaterialTheme.typography.titleSmall, maxLines = 1)
            Text(
                "from ${shortHex(welcome.inviterNpub)}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        TextButton(onClick = { model.declineWelcome(welcome.idHex) }) { Text("Decline") }
        Button(onClick = { model.acceptWelcome(welcome.idHex) }) { Text("Accept") }
    }
}

@Composable
private fun GroupRow(group: MarmotGroup, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Rust-derived avatar initials, rendered on a flat tile.
        Surface(
            modifier = Modifier.size(40.dp).clip(RoundedCornerShape(20.dp)),
            color = MaterialTheme.colorScheme.secondaryContainer,
        ) {
            Box(contentAlignment = Alignment.Center) {
                Text(group.initials, fontWeight = FontWeight.Bold)
            }
        }
        Spacer(Modifier.size(12.dp))
        Column(Modifier.weight(1f)) {
            Text(group.displayName, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold, maxLines = 1)
            Text(
                "${group.memberCount} members",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun CreateGroupDialog(onDismiss: () -> Unit, onCreate: (String, String) -> Unit) {
    var name by remember { mutableStateOf("") }
    var invitees by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New group") },
        text = {
            Column {
                TextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Group name") },
                    singleLine = true,
                )
                Spacer(Modifier.size(8.dp))
                TextField(
                    value = invitees,
                    onValueChange = { invitees = it },
                    label = { Text("Invite npubs (optional)") },
                    // Raw text — Rust tokenises and validates; no parsing here.
                )
            }
        },
        confirmButton = {
            Button(
                onClick = { onCreate(name.trim(), invitees.trim()) },
                enabled = name.trim().isNotEmpty(),
            ) { Text("Create") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

@Composable
private fun EmptyGroupsHint() {
    Text(
        "No groups yet. Tap + to create an encrypted group.",
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(16.dp),
    )
}

@Composable
private fun WarningBanner(text: String) {
    Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
        Text(
            text,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onErrorContainer,
            modifier = Modifier.padding(12.dp),
        )
    }
}

@Composable
private fun NotRegisteredState(subtitle: String) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("Groups unavailable", style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.size(8.dp))
            Text(
                // Rust-owned copy ("Sign in with an nsec to enable" when no
                // local key); rendered verbatim.
                subtitle.ifEmpty { "Encrypted groups require a local signing key." },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

/**
 * Render-grade hex shortening (presentation only; never decides behaviour).
 * `internal` so the sibling [GroupChatView] in `GroupChatScreen.kt` reuses the
 * single canonical copy rather than duplicating it (no fragmentation).
 */
internal fun shortHex(hex: String): String =
    if (hex.length >= 16) "${hex.take(8)}…${hex.takeLast(8)}" else hex
