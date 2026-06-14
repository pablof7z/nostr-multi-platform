package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ShortNoteProjectionEntry

/**
 * kind:1 short text note embed (#984). Android peer of iOS
 * `DefaultShortNoteRenderer`. Renders the kernel-resolved byline + plain-text
 * content. The `content` string was flattened from the NFCT sub-buffer by
 * [org.nmp.android.TypedEmbedSidecarDecoder] (D0: no re-parse here).
 */
@Composable
fun ShortNoteEmbedView(note: ShortNoteProjectionEntry) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = note.authorPubkey,
            authorDisplayName = note.authorDisplayName,
            caption = "note",
            avatarConsumerId = "embed-note-${note.id}",
        )
        if (note.content.isNotEmpty()) {
            Text(note.content, style = MaterialTheme.typography.bodyMedium)
        }
        note.mediaUrls.forEach { url ->
            Text(
                url,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}
