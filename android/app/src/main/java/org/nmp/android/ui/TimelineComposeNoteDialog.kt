package org.nmp.android.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Minimal compose note dialog: text input + send button.
 *
 * Routes the note through KernelModel.publishNote, which dispatches the
 * nmp.publish action and returns a correlation_id on success.
 */
@Composable
internal fun ComposeNoteDialog(
    onDismiss: () -> Unit,
    onPublish: (content: String) -> Unit,
    title: String = "New Note",
    inputLabel: String = "What's happening?",
    confirmLabel: String = "Publish",
) {
    var content by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            TextField(
                value = content,
                onValueChange = { content = it },
                label = { Text(inputLabel) },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(8.dp),
                maxLines = 8,
            )
        },
        confirmButton = {
            Button(
                onClick = {
                    if (content.isNotBlank()) {
                        onPublish(content)
                    }
                },
                enabled = content.isNotBlank(),
            ) {
                Text(confirmLabel)
            }
        },
        dismissButton = {
            Button(onClick = onDismiss) {
                Text("Cancel")
            }
        },
    )
}
