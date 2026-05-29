package org.nmp.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.model.RelayStatus

/**
 * D6 + D8 observability surface — Android peer of iOS `DiagnosticsView`.
 * Every value is verbatim from the kernel JSON; the only computed string is
 * the relative "last snapshot" age (presentation-only, same as Swift).
 */
@Composable
fun DiagnosticsScreen(model: KernelModel, modifier: Modifier = Modifier) {
    val s by model.state.collectAsStateWithLifecycle()
    val count by model.snapshotCount.collectAsStateWithLifecycle()
    val lastAt by model.lastSnapshotAtMs.collectAsStateWithLifecycle()

    LazyColumn(modifier.fillMaxSize().padding(16.dp)) {
        item { SectionHeader("Kernel snapshot") }
        item { KV("rev", "${s.rev}") }
        item { KV("snapshots received", "$count") }
        item { KV("running", if (s.running) "yes" else "no") }
        item { KV("last snapshot", relativeAge(lastAt)) }
        item { KV("chirp cards", "${s.modularTimeline.cards.size}") }

        s.metrics?.let { m ->
            item { SectionHeader("Metrics") }
            item { KV("stored events", "${m.storedEvents}") }
            item { KV("visible items", "${m.visibleItems}") }
            item { KV("events RX", "${m.eventsRx}") }
            item { KV("update sequence", "${m.updateSequence}") }
        }

        item { SectionHeader("Relays") }
        if (s.relayStatuses.isEmpty()) {
            item { Text("No relay status yet", style = MaterialTheme.typography.bodySmall) }
        } else {
            items(s.relayStatuses, key = { "${it.role}:${it.relayUrl}" }) { RelayRow(it) }
        }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(top = 20.dp, bottom = 6.dp),
    )
    HorizontalDivider()
}

@Composable
private fun KV(key: String, value: String) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(key, style = MaterialTheme.typography.bodyMedium)
        Text(value, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun RelayRow(relay: RelayStatus) {
    Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(
            relay.relayUrl,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            Modifier.fillMaxWidth().padding(top = 2.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(relay.connection, style = MaterialTheme.typography.labelSmall)
            Text("auth: ${relay.auth}", style = MaterialTheme.typography.labelSmall)
            Text("${relay.activeWireSubscriptions} subs", style = MaterialTheme.typography.labelSmall)
        }
        HorizontalDivider(Modifier.padding(top = 6.dp))
    }
}

private fun relativeAge(lastAtMs: Long?): String {
    if (lastAtMs == null) return "never"
    val elapsed = (System.currentTimeMillis() - lastAtMs) / 1000
    return if (elapsed < 1) "just now" else "${elapsed}s ago"
}
