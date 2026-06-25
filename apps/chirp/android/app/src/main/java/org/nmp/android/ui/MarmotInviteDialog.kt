package org.nmp.android.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ActionLifecycleSnapshot
import org.nmp.android.model.MarmotGroup
import org.nmp.android.model.MarmotPendingOp

/**
 * Invite dialog — Android peer of iOS `MarmotInviteSheet.swift`.
 *
 * Dialog stays OPEN after dispatch until the kernel reports a terminal verdict:
 *
 *   1. [onInvite] fires; caller stashes the correlation id and passes [isWaiting]=true.
 *   2. While parked in `snapshot.pendingOps`, [pendingOpRow] carries the Rust-owned
 *      `displayLabel` rendered verbatim. No polling (D8).
 *   3. [lifecycle] reaching `"accepted"` for the correlation id → caller sets
 *      [isWaiting]=false and dismisses. [errorMessage] shown on `"failed"`.
 *
 * Thin-shell rule: ALL copy comes from Rust. Kotlin only gates dismiss.
 */
@Composable
internal fun MarmotInviteDialog(
    group: MarmotGroup,
    lifecycle: ActionLifecycleSnapshot?,
    /** Correlation id of the in-flight invite op, null when no op is pending. */
    pendingCid: String?,
    /** Pending op row from snapshot for our cid, null when not yet parked. */
    pendingOpRow: MarmotPendingOp?,
    /** Rust-owned error from terminal failure or snapshot.lastOpError. */
    errorMessage: String?,
    onDismiss: () -> Unit,
    /** Caller dispatches the invite and stashes the returned correlation id. */
    onInvite: (inviteeText: String) -> Unit,
    /** Called when the terminal "accepted" verdict lands; caller should dismiss. */
    onAccepted: () -> Unit,
) {
    val isWaiting = pendingCid != null

    // Resolve terminal verdict.
    LaunchedEffect(lifecycle, pendingCid) {
        val cid = pendingCid ?: return@LaunchedEffect
        val terminal = lifecycle?.recentTerminal?.firstOrNull { it.correlationId == cid }
            ?: return@LaunchedEffect
        if (terminal.stage == "accepted") onAccepted()
        // "failed" case: errorMessage is already derived by caller from terminal.reason.
    }

    var inviteeText by remember { mutableStateOf("") }
    val hasInvitee = inviteeText.isNotBlank()

    AlertDialog(
        onDismissRequest = { if (!isWaiting) onDismiss() },
        title = { Text("Invite to ${group.displayName}") },
        text = {
            Column {
                Text(
                    "Invitee npubs",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(4.dp))
                TextField(
                    value = inviteeText,
                    onValueChange = { if (!isWaiting) inviteeText = it },
                    placeholder = {
                        Text(
                            "npub1…, npub1… (comma or newline separated)",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 100.dp),
                    maxLines = 6,
                    singleLine = false,
                    enabled = !isWaiting,
                )
                // Pending row: Rust-owned displayLabel verbatim.
                if (pendingOpRow != null) {
                    Spacer(Modifier.height(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(modifier = Modifier.size(14.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.size(6.dp))
                        Text(pendingOpRow.displayLabel, style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                // Spinner while dispatched but not yet in pendingOps.
                if (isWaiting && pendingOpRow == null) {
                    Spacer(Modifier.height(8.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        CircularProgressIndicator(modifier = Modifier.size(14.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.size(6.dp))
                        Text("Sending…", style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                // Error message: Rust-owned reason verbatim.
                if (errorMessage != null) {
                    Spacer(Modifier.height(8.dp))
                    Text(errorMessage, style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = {
            Button(
                onClick = { onInvite(inviteeText.trim()) },
                enabled = hasInvitee && !isWaiting,
            ) {
                if (isWaiting) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                } else {
                    Text("Send invites")
                }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}
