package org.nmp.android.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel

@Composable
fun ThreadScreen(
    eventId: String,
    model: KernelModel,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    DisposableEffect(eventId) {
        model.openThread(eventId)
        onDispose {
            model.closeThread(eventId)
        }
    }

    val snapshot by model.state.collectAsStateWithLifecycle()
    val projections = snapshot.projections
    val cards = projections?.flatFeeds?.get("nmp.feed.thread.$eventId")?.cards ?: emptyList()
    val cardLookup = cards.associate { it.card.id to it.card }
    val resolvedProfiles = projections?.resolvedProfiles ?: emptyMap()

    CompositionLocalProvider(LocalResolvedProfiles provides resolvedProfiles) {
        Column(modifier.fillMaxSize()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onBack) {
                    Icon(
                        Icons.AutoMirrored.Filled.ArrowBack,
                        contentDescription = "Back to timeline",
                    )
                }
                Text(
                    "Thread",
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.size(40.dp))
            }

            HorizontalDivider()

            if (cards.isEmpty()) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(16.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        "No thread events yet",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxSize()) {
                    itemsIndexed(
                        cards,
                        key = { _, root -> root.card.id },
                    ) { index, root ->
                        NoteRow(
                            eventId = root.card.id,
                            items = emptyMap(),
                            cards = cardLookup,
                            model = model,
                        )
                        if (index < cards.lastIndex) {
                            HorizontalDivider(Modifier.padding(start = 56.dp))
                        }
                    }
                }
            }
        }
    }
}
