package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.DispatchResult
import org.nmp.android.KernelModel
import org.nmp.android.model.ActionLifecycleSnapshot
import org.nmp.android.model.MarmotSnapshot

/**
 * Marmot (MLS-over-Nostr encrypted groups) screen — Android peer of the iOS
 * `MarmotGroupsView` / `MarmotStore` surface.
 *
 * Thin-shell rule (aim.md §2): ZERO protocol logic here. State is read from the
 * `nmp.marmot.snapshot` / `nmp.marmot.messages` push projections; every write
 * is a [KernelModel] call that routes through `dispatch_action("nmp.marmot", …)`.
 *
 * Create-group and invite semantics (PR-3 of marmot-create-fix ladder):
 *   • On dispatch, stash the kernel-minted correlation id.
 *   • While the op is parked in `snapshot.pendingOps`, show the Rust-owned
 *     `displayLabel` verbatim. The snapshot is push-driven (D8 — no polling).
 *   • `action_lifecycle.recentTerminal` returning `"accepted"` → dismiss.
 *   • `"failed"` → show reason verbatim, keep dialog (user can retry/cancel).
 *   • `snapshot.lastOpError` shown as a warning banner. It clears only when
 *     Rust next emits `last_op_error = null` (there is no shell-side clear op).
 *
 * Row / dialog / banner composables live in the sibling `GroupListComponents.kt`
 * so neither file crosses the 500-LOC hard cap (AGENTS.md).
 */
@Composable
fun GroupsScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val s by model.state.collectAsStateWithLifecycle()
    val activeAccount = s.activeAccount

    LaunchedEffect(activeAccount) {
        if (activeAccount.isNotEmpty()) {
            model.registerMarmotIfNeeded(context.filesDir.path)
        }
    }

    val snapshot = s.projections?.marmotSnapshot ?: MarmotSnapshot()
    val messagesByGroup = s.projections?.marmotMessages ?: emptyMap()
    val lifecycle = s.projections?.actionLifecycle

    var selectedGroupId by remember { mutableStateOf<String?>(null) }

    Box(modifier.fillMaxSize()) {
        val selected = selectedGroupId
        if (selected != null) {
            val group = snapshot.groups.firstOrNull { it.idHex == selected }
            if (group == null) {
                selectedGroupId = null
            } else {
                GroupChatView(
                    model = model,
                    group = group,
                    messages = messagesByGroup[selected] ?: emptyList(),
                    onBack = { selectedGroupId = null },
                    hasOrphanedCommit = snapshot.orphanedCommitCount > 0,
                )
            }
        } else {
            GroupListScreen(
                model = model,
                snapshot = snapshot,
                lifecycle = lifecycle,
                onSelectGroup = { selectedGroupId = it },
            )
        }
    }
}

@Composable
private fun GroupListScreen(
    model: KernelModel,
    snapshot: MarmotSnapshot,
    lifecycle: ActionLifecycleSnapshot?,
    onSelectGroup: (String) -> Unit,
) {
    var showCreate by remember { mutableStateOf(false) }
    // Correlation id from the most recent create dispatch.
    var createCid by remember { mutableStateOf<String?>(null) }
    // Terminal failure reason captured at resolution time (Rust-owned, verbatim).
    var createError by remember { mutableStateOf<String?>(null) }

    // Resolve terminal verdict for the stashed create correlation id.
    LaunchedEffect(lifecycle, createCid) {
        val cid = createCid ?: return@LaunchedEffect
        val terminal = lifecycle?.recentTerminal?.firstOrNull { it.correlationId == cid }
            ?: return@LaunchedEffect
        createCid = null
        if (terminal.stage == "accepted") {
            createError = null
            showCreate = false
        } else {
            // "failed": keep dialog open; surface the localized reason_code,
            // falling back to the Rust prose `reason` when un-coded (#1735).
            createError = terminal.localizedReason
        }
    }

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

            // #1651 service-init failure (replaces the V-62 keyringUnavailable
            // bool). Raw machine token from Rust → shell-owned copy (aim.md §2):
            // a minimal diagnostic, not a recovery flow. Rendered BEFORE the
            // not-registered early-return: the InitFailed states (DbKeyLost /
            // Other / KeyringUnavailable) are intentionally is_registered=false,
            // so gating this behind isRegistered would suppress the very failure
            // it surfaces (mirrors iOS MarmotGroupsView, which renders it first).
            when (snapshot.initErrorKind) {
                "keyring_unavailable" -> WarningBanner(
                    "Keyring unavailable — group secrets are kept in memory only and " +
                        "will be lost on next launch.",
                )
                "db_key_lost" -> WarningBanner(
                    "Encrypted groups unavailable: the encrypted message database key " +
                        "was lost; encrypted groups are unavailable.",
                )
                "init_failed" -> WarningBanner(
                    "Encrypted groups unavailable: the encrypted message database could " +
                        "not be opened. Free up space or check storage permissions, then relaunch.",
                )
            }

            if (!snapshot.isRegistered) {
                // Shell owns this copy now (aim.md §2 — presentation in the shell).
                // keyPackageSubtitle() returns the not-registered prose when the
                // key package's isRegistered flag is false.
                NotRegisteredState(subtitle = keyPackageSubtitle(snapshot.keyPackage))
                return@Column
            }

            // Last-op-error banner — Rust-owned (op, reason) machine codes mapped
            // to a banner by marmotErrorBanner (aim.md §2 sanctioned mapping).
            // Informational: clears only when Rust next emits last_op_error = null
            // (Rust clears it on the next successful op; no shell-side clear op).
            snapshot.lastOpError?.let { err ->
                WarningBanner(marmotErrorBanner(err))
            }
            if (snapshot.orphanedCommitCount > 0) {
                WarningBanner(
                    "A group commit may not have reached the relay. Sending is blocked " +
                        "until the group recovers.",
                )
            }

            LazyColumn(Modifier.fillMaxSize()) {
                item { KeyPackageRow(model, snapshot) }

                // Pending ops — Rust-owned displayLabel rendered verbatim.
                if (snapshot.pendingOps.isNotEmpty()) {
                    item { GroupSectionHeader("In progress") }
                    items(snapshot.pendingOps, key = { it.correlationId }) { op ->
                        PendingOpRow(op)
                        HorizontalDivider()
                    }
                }

                if (snapshot.pendingWelcomes.isNotEmpty()) {
                    item { GroupSectionHeader("Invites") }
                    items(snapshot.pendingWelcomes, key = { it.idHex }) { welcome ->
                        PendingWelcomeRow(model, welcome)
                        HorizontalDivider()
                    }
                }

                item { GroupSectionHeader("Your groups") }
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
        // Pending op for the active cid (null when no cid stashed yet or when
        // the op has not entered the parked state).
        val pendingOpRow = createCid?.let { cid ->
            snapshot.pendingOps.firstOrNull { it.correlationId == cid }
        }
        val isWaiting = createCid != null
        CreateGroupDialog(
            isWaiting = isWaiting,
            pendingOpRow = pendingOpRow,
            // Terminal failure reason captured by the LaunchedEffect; fall back
            // to the mapped snapshot.lastOpError only when idle (e.g. sync reject).
            errorMessage = createError
                ?: if (!isWaiting) snapshot.lastOpError?.let { marmotErrorBanner(it) } else null,
            onDismiss = {
                createCid = null
                createError = null
                showCreate = false
            },
            onCreate = { name, invitees ->
                createError = null
                val result = model.marmot.createGroup(
                    name = name, description = "", inviteeText = invitees)
                createCid = if (result is DispatchResult.Accepted) {
                    result.correlationId
                } else {
                    // Synchronous rejection — keep dialog open. The LaunchedEffect
                    // will not fire (no cid), but lastOpError from snapshot surfaces
                    // any Rust-side rejection reason on the next tick.
                    null
                }
            },
        )
    }
}
