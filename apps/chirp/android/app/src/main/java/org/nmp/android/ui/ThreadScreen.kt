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
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.ui.embed.EventClaimer
import org.nmp.android.ui.embed.LocalRefEventEnvelopes
import org.nmp.android.ui.embed.LocalEventClaimer

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
    val profileHost = rememberKernelProfileHost(model)
    var replyContent by remember(eventId) { mutableStateOf("") }

    // Provide the on-demand claimer at the thread root so `RememberProfileClaim`
    // calls in thread author rows are live (not no-ops) — mirrors TimelineScreen
    // and DmConversationListScreen. Without it, thread author display names never
    // trigger an on-demand kind:0 fetch (issue #1303). ADR-0063 Lane G: feed/list
    // fetches resolve the ProfileRef shape with CacheOk liveness.
    val claimer: ProfileClaimer = { pubkey, consumerId, claim ->
        if (claim) model.resolveProfile(pubkey, consumerId)
        else model.releaseProfile(pubkey, consumerId)
    }
    val eventClaimer: EventClaimer = { uri, consumerId, claim ->
        if (claim) model.claimEvent(uri, consumerId)
        else model.releaseEvent(uri, consumerId)
    }
    val refEventEnvelopes = projections?.refEventEnvelopes ?: emptyMap()

    CompositionLocalProvider(
        LocalProfileClaimer provides claimer,
        LocalEventClaimer provides eventClaimer,
        LocalRefEventEnvelopes provides refEventEnvelopes,
        LocalNostrProfileHost provides profileHost,
    ) {
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
                        .weight(1f)
                        .fillMaxWidth()
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
                LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
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

            HorizontalDivider()
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextField(
                    value = replyContent,
                    onValueChange = { replyContent = it },
                    label = { Text("Write a reply") },
                    modifier = Modifier.weight(1f),
                    maxLines = 4,
                )
                Spacer(Modifier.size(8.dp))
                Button(
                    onClick = {
                        val content = replyContent.trim()
                        if (content.isNotEmpty() && cardLookup.containsKey(eventId)) {
                            model.publishNote(content, eventId)
                            replyContent = ""
                        }
                    },
                    // M14-1 / #2145: `publishReply` looks up the STORED parent
                    // event in the kernel to derive the NIP-10 tags and rejects a
                    // missing/non-kind:1 parent. Gate the reply until the parent
                    // card is present in the thread feed (it always is for a
                    // thread opened from a rendered note; the empty/pending case
                    // after a search/deep-link is the one we guard) so we never
                    // submit a reply the kernel would reject as
                    // `reply_target_unknown`.
                    enabled = replyContent.isNotBlank() && cardLookup.containsKey(eventId),
                ) {
                    Text("Reply")
                }
            }
        }
    }
}
