package org.nmp.gallery.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.gallery.model.GalleryBundle
import org.nmp.gallery.model.Scenario
import org.nmp.gallery.model.SignedEventJson

/**
 * Scrollable labelled gallery — one cell per scenario. Compose port of
 * Swift `GalleryView`. Each cell shows title, exercises subtitle, the
 * NMP-rendered output, and a collapsible "Event JSON" disclosure.
 */
@Composable
fun GalleryScreen(bundle: GalleryBundle, modifier: Modifier = Modifier) {
    val categories = remember(bundle) {
        bundle.scenarios.map { it.category }.distinct()
    }

    Surface(
        modifier = modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "NMP Content Gallery",
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    "v${bundle.version}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            HorizontalDivider()

            LazyColumn(modifier = Modifier.fillMaxSize()) {
                item {
                    Text(
                        "${bundle.scenarios.count()} scenarios · bundle v${bundle.version}",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(16.dp),
                    )
                }
                for (category in categories) {
                    item(key = "h-$category") {
                        Text(
                            category.replaceFirstChar(Char::titlecase),
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(Color.Gray.copy(alpha = 0.10f))
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                        )
                    }
                    val scenarios = bundle.scenarios.filter { it.category == category }
                    items(scenarios, key = { it.id }) { scenario ->
                        ScenarioCell(scenario)
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

@Composable
private fun ScenarioCell(scenario: Scenario) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                scenario.id,
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
                modifier = Modifier
                    .clip(RoundedCornerShape(percent = 50))
                    .background(SwiftAccent.copy(alpha = 0.15f))
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            )
            Text(
                scenario.title,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
            )
        }
        Text(
            scenario.exercises,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .background(Color.Gray.copy(alpha = 0.06f))
                .padding(10.dp),
        ) {
            ScenarioRenderer(
                rendered = scenario.rendered,
                embeds = scenario.embeds,
            )
        }

        EventJsonDisclosure(scenario.events)
    }
}

@Composable
private fun EventJsonDisclosure(events: List<SignedEventJson>) {
    var expanded by remember { mutableStateOf(false) }
    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { expanded = !expanded }
                .padding(vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(if (expanded) "▾" else "▸", style = MaterialTheme.typography.labelMedium)
            Text(
                "Event JSON",
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
        }
        if (expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (ev in events) {
                    Text(
                        text = eventSummary(ev),
                        style = MaterialTheme.typography.labelSmall.copy(
                            fontFamily = FontFamily.Monospace,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }
    }
}

private fun eventSummary(ev: SignedEventJson): String = buildString {
    append("kind ").append(ev.kind).append(" · id ").append(ev.id.take(12)).append("…\n")
    append("pubkey ").append(ev.pubkey.take(12)).append("… · sig ")
    append(ev.sig.take(12)).append("…\n")
    append("content: ").append(ev.content.take(120))
}
