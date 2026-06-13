package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.DispatchResult
import org.nmp.android.KernelModel
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotMessage
import org.nmp.android.model.MarmotSnapshot

/**
 * One Marmot (MLS) group thread — Android peer of iOS `MarmotGroupChatView`.
 * Renders the decrypted message stream from the `nmp.marmot.messages` push
 * projection (passed in by [GroupsScreen]) and a compose row.
 *
 * Thin-shell rule (aim.md §2): no protocol logic. Sending routes through
 * [MarmotActions.sendGroupMessage] → `dispatch_action("nmp.marmot", {"op":"send",…})`;
 * the sent message reappears on the next snapshot tick via the push projection
 * (D8 — no poll, no optimistic local echo).
 *
 * The overflow menu wires the iOS `MarmotBridge` op surface (Invite, Remove,
 * Leave, clear-pending). The Android UI additionally exposes Remove (per-member),
 * clear-pending, and a leave-confirm dialog that iOS exposes only at the bridge —
 * iOS surfaces only Invite + (unconfirmed) Leave in its `toolbarContent`.
 */
@Composable
internal fun GroupChatView(
    model: KernelModel,
    group: MarmotGroup,
    messages: List<MarmotMessage>,
    onBack: () -> Unit,
    hasOrphanedCommit: Boolean = false,
) {
    val s by model.state.collectAsStateWithLifecycle()
    val snapshot = s.projections?.marmotSnapshot ?: MarmotSnapshot()
    val lifecycle = s.projections?.actionLifecycle

    var draft by remember { mutableStateOf("") }
    var showMenu by remember { mutableStateOf(false) }
    var showInvite by remember { mutableStateOf(false) }
    var inviteCid by remember { mutableStateOf<String?>(null) }
    var inviteError by remember { mutableStateOf<String?>(null) }
    var showMembers by remember { mutableStateOf(false) }
    var showLeaveConfirm by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize()) {
        // Header row
        Row(
            Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            TextButton(onClick = onBack) { Text("Back") }
            Column(Modifier.weight(1f)) {
                Text(
                    group.displayName,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                )
                Text(
                    "${group.memberCount} ${if (group.memberCount == 1) "member" else "members"}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            // Overflow menu — wires the iOS MarmotBridge op surface (iOS UI
            // surfaces only Invite + unconfirmed Leave in its toolbarContent).
            Box {
                IconButton(onClick = { showMenu = true }) {
                    Icon(Icons.Filled.MoreVert, contentDescription = "Group options")
                }
                DropdownMenu(
                    expanded = showMenu,
                    onDismissRequest = { showMenu = false },
                ) {
                    DropdownMenuItem(
                        text = { Text("Invite members") },
                        onClick = { showMenu = false; showInvite = true },
                    )
                    DropdownMenuItem(
                        text = { Text("Members") },
                        onClick = { showMenu = false; showMembers = true },
                    )
                    DropdownMenuItem(
                        text = {
                            Text(
                                "Leave group",
                                color = MaterialTheme.colorScheme.error,
                            )
                        },
                        onClick = { showMenu = false; showLeaveConfirm = true },
                    )
                }
            }
        }
        HorizontalDivider()

        // Pending-commit failure affordance. Wires the iOS MarmotBridge.clearPending
        // op (unwired in iOS UI) into a Clear-pending button. Surfaced when the
        // snapshot signals orphaned commits — matches the GroupListScreen
        // WarningBanner gate (snapshot.orphanedCommitCount > 0), forwarded by caller.
        if (hasOrphanedCommit) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    "A commit may not have reached the relay.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { model.marmot.clearPending(group.idHex) }) {
                    Text("Clear pending")
                }
            }
            HorizontalDivider()
        }

        // Message stream
        if (messages.isEmpty()) {
            Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                Text(
                    "No messages yet\nSend an encrypted message to start the conversation.",
                    textAlign = TextAlign.Center,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(24.dp),
                )
            }
        } else {
            LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(vertical = 8.dp)) {
                itemsIndexed(messages, key = { _, m -> m.id }) { _, message ->
                    GroupMessageBubble(message)
                }
            }
        }

        HorizontalDivider()
        // Composer row
        Row(
            Modifier.fillMaxWidth().padding(8.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            androidx.compose.material3.TextField(
                value = draft,
                onValueChange = { draft = it },
                label = { Text("Encrypted message…") },
                modifier = Modifier.weight(1f).clip(RoundedCornerShape(8.dp)),
                maxLines = 3,
                colors = androidx.compose.material3.TextFieldDefaults.colors(
                    unfocusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                    focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                ),
            )
            IconButton(
                onClick = {
                    val trimmed = draft.trim()
                    if (trimmed.isNotEmpty()) {
                        model.marmot.sendGroupMessage(group.idHex, trimmed)
                        draft = ""
                    }
                },
                enabled = draft.trim().isNotEmpty(),
            ) {
                Icon(Icons.Filled.Send, contentDescription = "Send")
            }
        }
    }

    // Invite dialog — stays open until terminal verdict (PR-3 semantics).
    if (showInvite) {
        val pendingOpRow = inviteCid?.let { cid ->
            snapshot.pendingOps.firstOrNull { it.correlationId == cid }
        }
        // Show terminal failure reason, or the mapped snapshot.lastOpError when
        // no cid is in flight (marmotErrorBanner — aim.md §2 sanctioned mapping).
        val errorMsg = inviteError
            ?: if (inviteCid == null) snapshot.lastOpError?.let { marmotErrorBanner(it) } else null
        MarmotInviteDialog(
            group = group,
            lifecycle = lifecycle,
            pendingCid = inviteCid,
            pendingOpRow = pendingOpRow,
            errorMessage = errorMsg,
            onDismiss = {
                inviteCid = null
                inviteError = null
                showInvite = false
            },
            onInvite = { inviteeText ->
                inviteError = null
                val result = model.marmot.invite(group.idHex, inviteeText)
                when (result) {
                    is DispatchResult.Accepted -> inviteCid = result.correlationId
                    is DispatchResult.Failure -> inviteError = result.message
                }
            },
            onAccepted = {
                inviteCid = null
                inviteError = null
                showInvite = false
            },
        )
    }

    // Members sheet — per-member Remove. Wires the iOS MarmotBridge.remove op
    // (unwired in iOS UI; iOS MarmotMembersSheet is read-only) into the list.
    if (showMembers) {
        MarmotMembersDialog(
            members = group.members,
            onRemove = { memberHex ->
                model.marmot.removeMembers(group.idHex, listOf(memberHex))
            },
            onDismiss = { showMembers = false },
        )
    }

    // Leave-group confirmation. Android adds a confirm dialog that iOS lacks:
    // iOS MarmotGroupChatView.swift:202-209 dispatches leave directly with NO
    // confirmation. This guards an irreversible MLS SelfRemove behind a prompt.
    if (showLeaveConfirm) {
        AlertDialog(
            onDismissRequest = { showLeaveConfirm = false },
            title = { Text("Leave group") },
            text = { Text("Are you sure you want to leave “${group.displayName}”? This cannot be undone.") },
            confirmButton = {
                Button(
                    onClick = {
                        showLeaveConfirm = false
                        model.marmot.leave(group.idHex)
                        onBack()
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                    ),
                ) { Text("Leave") }
            },
            dismissButton = {
                TextButton(onClick = { showLeaveConfirm = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun GroupMessageBubble(message: MarmotMessage) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp)) {
        Text(
            // Single canonical hex-shortener lives in GroupsScreen.kt.
            shortHex(message.senderPubkeyHex),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier.padding(vertical = 2.dp),
        ) {
            Text(
                message.content,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
            )
        }
    }
}

/**
 * Members sheet with per-member Remove action. Wires the iOS
 * `MarmotBridge.remove` op (unwired in iOS UI — the iOS `MarmotMembersSheet`
 * is read-only) into the members list.
 */
@Composable
internal fun MarmotMembersDialog(
    members: List<String>,
    onRemove: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Members") },
        text = {
            LazyColumn {
                itemsIndexed(members, key = { _, m -> m }) { _, member ->
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            shortHex(member),
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.weight(1f),
                        )
                        TextButton(
                            onClick = { onRemove(member) },
                            colors = ButtonDefaults.textButtonColors(
                                contentColor = MaterialTheme.colorScheme.error,
                            ),
                        ) { Text("Remove") }
                    }
                    HorizontalDivider()
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text("Done") }
        },
    )
}
