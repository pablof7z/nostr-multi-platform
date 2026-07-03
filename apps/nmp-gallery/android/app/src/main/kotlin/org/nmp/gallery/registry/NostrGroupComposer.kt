package org.nmp.gallery.registry

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun NostrGroupComposer(
    text: String,
    onTextChange: (String) -> Unit,
    onSend: (String) -> Unit,
    modifier: Modifier = Modifier,
    placeholder: String = "Message",
    enabled: Boolean = true,
) {
    val trimmed = text.trim()
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = text,
            onValueChange = onTextChange,
            enabled = enabled,
            placeholder = { Text(placeholder) },
            minLines = 1,
            maxLines = 5,
            modifier = Modifier
                .weight(1f)
                .padding(end = 8.dp),
        )
        Button(
            enabled = enabled && trimmed.isNotEmpty(),
            onClick = {
                if (trimmed.isNotEmpty()) {
                    onSend(trimmed)
                    onTextChange("")
                }
            },
        ) {
            Text("Send")
        }
    }
}
