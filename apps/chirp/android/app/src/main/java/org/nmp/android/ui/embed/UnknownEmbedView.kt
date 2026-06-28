package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.nmp.android.model.UnknownProjectionEntry

/**
 * Fallback embed renderer for numeric kinds without a typed projection (#984).
 * Android peer of iOS `DefaultUnknownRenderer`. The kernel pre-bucketed the
 * event into [UnknownProjectionEntry] (alt-text / content already extracted);
 * Kotlin renders verbatim — it never inspects [UnknownProjectionEntry.kind] to
 * decide *how* to render (that classification is the kernel's job).
 */
@Composable
fun UnknownEmbedView(unknown: UnknownProjectionEntry) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = unknown.authorPubkey,
            authorDisplayName = unknown.authorDisplayName,
            caption = "kind ${unknown.kind}",
            avatarConsumerId = "embed-unknown-${unknown.authorPubkey}-${unknown.kind}",
        )
        val body = unknown.altText?.takeIf { it.isNotEmpty() }
            ?: unknown.content.takeIf { it.isNotEmpty() }
        body?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
