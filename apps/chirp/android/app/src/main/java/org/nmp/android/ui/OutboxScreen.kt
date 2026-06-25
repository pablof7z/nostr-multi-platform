package org.nmp.android.ui

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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.ackActionStage
import org.nmp.android.cancelPublish
import org.nmp.android.retryPublish
import org.nmp.android.model.ActionStageEntry

/**
 * Publish-outbox screen (#1291 GAP 4) — Android peer of the chirp-desktop
 * `outbox_panel`. Renders the kernel-owned `action_stages` projection
 * (correlation_id → latest stage) with per-row Retry / Cancel affordances and
 * acks terminal stages so the kernel evicts them from the sidecar.
 *
 * Thin-shell rule: Rust owns the publish ledger and retry/cancel/eviction
 * policy. This screen renders the projection verbatim and forwards control-plane
 * intents through the existing [KernelModel] methods — no local bookkeeping.
 */
@Composable
fun OutboxScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val state by model.state.collectAsStateWithLifecycle()
    // Latest stage per correlation id (the kernel appends stages in order).
    val rows = state.projections?.actionStages
        ?.mapNotNull { (correlationId, stages) ->
            stages.lastOrNull()?.let { correlationId to it }
        }
        ?: emptyList()

    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Publish Outbox", style = MaterialTheme.typography.headlineSmall)
                Text(
                    "${rows.size} pending",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            HorizontalDivider()

            if (rows.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        "No pending publishes",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    items(rows, key = { it.first }) { (correlationId, stage) ->
                        OutboxRow(
                            correlationId = correlationId,
                            stage = stage,
                            onRetry = { model.retryPublish(correlationId) },
                            onCancel = { model.cancelPublish(correlationId) },
                            onAckTerminal = { model.ackActionStage(correlationId) },
                        )
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

/** Terminal stages mirror the desktop panel's set. `cancelled` is the
 *  user-initiated terminal (S7/#1754), distinct from `failed`/`error`. */
private fun isTerminal(stage: String): Boolean =
    stage == "published" || stage == "failed" || stage == "error" || stage == "cancelled"

private fun stageColor(stage: String): Color = when (stage) {
    "publishing" -> Color(0xFFF97316)
    "published" -> Color(0xFF4ADE80)
    "failed", "error" -> Color(0xFFF87171)
    // S7/#1754: a user-initiated cancel is neutral, not an error red.
    "cancelled" -> Color(0xFF94A3B8)
    else -> Color(0xFF94A3B8)
}

@Composable
private fun OutboxRow(
    correlationId: String,
    stage: ActionStageEntry,
    onRetry: () -> Unit,
    onCancel: () -> Unit,
    onAckTerminal: () -> Unit,
) {
    val terminal = isTerminal(stage.stage)
    // Ack terminal stages once shown so the kernel evicts them and the sidecar
    // stops accumulating entries (mirrors the desktop panel). Keyed on the
    // (id, stage) pair so a re-entry to the same terminal stage re-acks.
    if (terminal) {
        LaunchedEffect(correlationId, stage.stage) { onAckTerminal() }
    }

    val shortId = if (correlationId.length > 16) {
        "${correlationId.take(13)}…"
    } else {
        correlationId
    }

    Column(
        Modifier
            .fillMaxWidth()
            .padding(12.dp)
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                shortId,
                style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
            )
            Text(
                stage.stage,
                style = MaterialTheme.typography.labelMedium,
                color = stageColor(stage.stage),
            )
        }
        stage.reason?.takeIf { it.isNotEmpty() }?.let { reason ->
            Spacer(Modifier.size(4.dp))
            Text(
                reason,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.size(6.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onRetry) { Text("Retry") }
            TextButton(onClick = onCancel) { Text("Cancel") }
        }
    }
}
