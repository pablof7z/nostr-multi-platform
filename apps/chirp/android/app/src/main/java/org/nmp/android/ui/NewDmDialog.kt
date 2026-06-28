package org.nmp.android.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
internal fun NewDmDialog(
    onDismiss: () -> Unit,
    onSend: (recipientPubkey: String, content: String) -> Unit,
) {
    var recipient by remember { mutableStateOf("") }
    var content by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New DM") },
        text = {
            Column {
                TextField(
                    value = recipient,
                    onValueChange = { recipient = it },
                    label = { Text("Recipient pubkey") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Spacer(Modifier.size(8.dp))
                TextField(
                    value = content,
                    onValueChange = { content = it },
                    label = { Text("Message") },
                    modifier = Modifier.fillMaxWidth(),
                    maxLines = 4,
                )
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    onSend(recipient.trim(), content.trim())
                },
                enabled = recipient.isNotBlank() && content.isNotBlank(),
            ) {
                Text("Send")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        },
    )
}
