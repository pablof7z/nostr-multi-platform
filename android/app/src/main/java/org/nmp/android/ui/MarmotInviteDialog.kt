package org.nmp.android.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import org.nmp.android.model.MarmotGroup

/**
 * Invite dialog — Android peer of iOS `MarmotInviteSheet.swift`.
 *
 * Presents a free-text field for npubs (comma or newline separated). Rust
 * tokenises and validates each entry on dispatch; Kotlin does ZERO parsing.
 * Empty input disables the confirm button (mirrors iOS `hasInviteeText` guard).
 *
 * Displayed from [GroupChatView]'s overflow menu "Invite members" item.
 */
@Composable
internal fun MarmotInviteDialog(
    group: MarmotGroup,
    onDismiss: () -> Unit,
    onInvite: (inviteeText: String) -> Unit,
) {
    var inviteeText by remember { mutableStateOf("") }
    val hasInvitee = inviteeText.isNotBlank()

    AlertDialog(
        onDismissRequest = onDismiss,
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
                    onValueChange = { inviteeText = it },
                    // Raw text — Rust tokenises on whitespace / comma / semicolon /
                    // newline and validates each npub. No parsing in Kotlin.
                    placeholder = {
                        Text(
                            "npub1…, npub1… (comma or newline separated)",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    },
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 100.dp),
                    maxLines = 6,
                    singleLine = false,
                )
            }
        },
        confirmButton = {
            Button(
                onClick = { onInvite(inviteeText.trim()) },
                enabled = hasInvitee,
            ) { Text("Send invites") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}
