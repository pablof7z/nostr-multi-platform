package org.nmp.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.android.KernelModel
import org.nmp.android.model.ChirpEventCard

/**
 * Note-row social action bar (#1291 GAP 1). Reply opens the existing compose
 * path with the card id as `replyToId`; React, Repost, and Zap dispatch the
 * EXISTING [KernelModel] actions. All policy lives in Rust — these are thin
 * call sites. Mirrors iOS `HomeFeedView`'s action row.
 *
 * Zap is shown unconditionally: the recipient `lnurl` is resolved kernel-side
 * from the author's kind:0 (the Android card model carries no `authorLnurl`),
 * and a missing LN address fails closed in Rust rather than in the shell.
 *
 * Split out of TimelineScreen.kt to keep that file under the 500-LOC ceiling
 * (AGENTS.md File Size).
 */
@Composable
internal fun NoteActionsSummary(card: ChirpEventCard?, model: KernelModel?) {
    val counts = card?.relationCounts ?: return
    var showReplyDialog by remember(card.id) { mutableStateOf(false) }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        if (model != null && card.id.isNotEmpty()) {
            RelationActionLabel("Reply", counts.replies.value) {
                showReplyDialog = true
            }
            RelationActionLabel("React", counts.reactions.value) {
                // "❤" to match iOS HomeFeedView default reaction.
                model.react(card.id, "❤")
            }
            RelationActionLabel("Repost", counts.reposts.value) {
                if (card.authorPubkey.isNotEmpty()) {
                    model.repost(card.id, card.authorPubkey)
                }
            }
            RelationActionLabel("Zap", counts.zaps.value, muted = true) {
                if (card.authorPubkey.isNotEmpty()) {
                    model.zapNote(card.id, card.authorPubkey, 21000L, "")
                }
            }
        } else {
            RelationCountLabel("Reply", counts.replies.value)
            RelationCountLabel("React", counts.reactions.value)
            RelationCountLabel("Repost", counts.reposts.value)
            RelationCountLabel("Zap", counts.zaps.value, muted = true)
        }
    }

    if (showReplyDialog && model != null && card.id.isNotEmpty()) {
        ComposeNoteDialog(
            title = "Reply",
            inputLabel = "Write a reply",
            confirmLabel = "Reply",
            onDismiss = { showReplyDialog = false },
            onPublish = { content ->
                model.publishNote(content, card.id)
                showReplyDialog = false
            },
        )
    }
}

/** Tappable variant of [RelationCountLabel] that dispatches [onClick]. */
@Composable
private fun RelationActionLabel(
    label: String,
    count: ULong?,
    muted: Boolean = false,
    onClick: () -> Unit,
) {
    Text(
        "$label ${count?.toString() ?: "..."}",
        style = MaterialTheme.typography.labelSmall,
        color = if (muted) {
            MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.72f)
        } else {
            MaterialTheme.colorScheme.primary
        },
        modifier = Modifier.clickable(onClick = onClick),
    )
}

@Composable
private fun RelationCountLabel(label: String, count: ULong?, muted: Boolean = false) {
    Text(
        "$label ${count?.toString() ?: "..."}",
        style = MaterialTheme.typography.labelSmall,
        color = if (muted) {
            MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.72f)
        } else {
            MaterialTheme.colorScheme.onSurfaceVariant
        },
    )
}
