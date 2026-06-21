package org.nmp.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
import org.nmp.android.RefLiveness
import org.nmp.android.RefShape
import org.nmp.android.sendDm
import org.nmp.android.components.LocalNostrProfileHost
import org.nmp.android.components.NostrAvatar
import org.nmp.android.model.DmConversation
import org.nmp.android.model.DmInboxSnapshot
import org.nmp.android.model.DmMessage

/**
 * NIP-17 direct-message conversations screen — Android peer of iOS `DmListView`.
 *
 * Reads the `nmp.nip17.dm_inbox` projection from the kernel snapshot.
 * Renders a list of conversations (newest-thread-first) or an empty-state
 * placeholder when no DM data is available.
 *
 * Thin-shell rule: ZERO protocol logic here. Conversations arrive
 * newest-thread-first from the Rust `DmInboxProjection`; this view only
 * renders the list and navigates into a thread.
 */
@Composable
fun DmScreen(model: KernelModel, modifier: Modifier = Modifier) {
    // Single collection of the kernel snapshot for this screen subtree (the DM
    // inbox). ADR-0063 Lane G: author profiles are read per-key via the
    // `refs.profile` keyed-ref cache through the profile host, not a snapshot map.
    val s by model.state.collectAsStateWithLifecycle()
    val dmInbox = s.projections?.dmInbox ?: DmInboxSnapshot()

    var selectedPeerPubkey by remember { mutableStateOf<String?>(null) }
    var showNewDmDialog by remember { mutableStateOf(false) }

    Box(modifier.fillMaxSize()) {
        if (selectedPeerPubkey != null) {
            DmConversationView(
                model = model,
                peerPubkey = selectedPeerPubkey!!,
                onBack = { selectedPeerPubkey = null }
            )
        } else {
            DmConversationListScreen(
                model = model,
                dmInbox = dmInbox,
                onSelectConversation = { pubkey -> selectedPeerPubkey = pubkey },
                onStartConversation = { showNewDmDialog = true },
            )
        }
        if (showNewDmDialog) {
            NewDmDialog(
                onDismiss = { showNewDmDialog = false },
                onSend = { recipient, content ->
                    model.sendDm(recipient, content)
                    selectedPeerPubkey = recipient
                    showNewDmDialog = false
                },
            )
        }
    }
}

/**
 * The conversation list (newest-thread-first).
 */
@Composable
private fun DmConversationListScreen(
    model: KernelModel,
    dmInbox: DmInboxSnapshot,
    onSelectConversation: (String) -> Unit,
    onStartConversation: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // ADR-0063 Lane G: list-row avatar fetches resolve ProfileRef / CacheOk.
    val claimer: ProfileClaimer = { pubkey, consumerId, claim ->
        if (claim) model.resolveProfile(pubkey, consumerId)
        else model.releaseProfile(pubkey, consumerId)
    }
    val profileHost = rememberKernelProfileHost(model)
    CompositionLocalProvider(
        LocalProfileClaimer provides claimer,
        LocalNostrProfileHost provides profileHost,
    ) {
        Column(modifier.fillMaxSize()) {
            Row(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Chats", style = MaterialTheme.typography.headlineSmall)
                Button(
                    onClick = onStartConversation,
                    enabled = !dmInbox.isUnavailable,
                ) {
                    Text("New")
                }
            }
            HorizontalDivider()

            // §D7: "unavailable" (no active account) hides the screen; "limited"
            // (bunker backfill pending/throttled by the bounded per-account
            // decrypt queue) renders the list WITH a "still decrypting" banner
            // rather than hiding pending messages (errors-as-state).
            if (dmInbox.isUnavailable) {
                UnavailableDmState()
            } else if (dmInbox.conversations.isEmpty() && !dmInbox.isLimited) {
                EmptyDmState()
            } else {
                ConversationListContent(
                    conversations = dmInbox.conversations,
                    decryptingCount = if (dmInbox.isLimited) dmInbox.undecryptedCount else 0,
                    onSelectConversation = onSelectConversation
                )
            }
        }
    }
}

/**
 * The conversation list content (LazyColumn).
 */
@Composable
private fun ConversationListContent(
    conversations: List<DmConversation>,
    onSelectConversation: (String) -> Unit,
    modifier: Modifier = Modifier,
    decryptingCount: Int = 0,
) {
    LazyColumn(modifier.fillMaxSize()) {
        // §D7 "limited" banner — a bunker backfill is pending or throttled by
        // the bounded per-account decrypt queue. Surfaced as state (the count
        // is never silently dropped), shown above whatever already decrypted.
        if (decryptingCount > 0) {
            item(key = "dm-decrypting-banner") {
                DecryptingBanner(count = decryptingCount)
                HorizontalDivider()
            }
        }
        itemsIndexed(conversations, key = { _, conv -> conv.peerPubkey }) { _, conversation ->
            DmConversationRow(
                conversation = conversation,
                onClick = { onSelectConversation(conversation.peerPubkey) }
            )
            HorizontalDivider()
        }
    }
}

/**
 * ADR-0050 §D7 "limited" banner: N envelopes are still decrypting (bunker
 * backfill pending or throttled by the bounded per-account decrypt queue).
 */
@Composable
private fun DecryptingBanner(count: Int) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
        Spacer(Modifier.size(10.dp))
        Text(
            if (count == 1) "1 message still decrypting…" else "$count messages still decrypting…",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * One row in the DM conversation list.
 * Displays the peer's pubkey (shortened), last message preview, and timestamp.
 */
@Composable
private fun DmConversationRow(
    conversation: DmConversation,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // NostrAvatar below is self-claiming via LocalNostrProfileHost, so no manual
    // RememberProfileClaim is needed for the peer avatar here.
    val latest = conversation.messages.lastOrNull()
    val peerShortHex = shortPubkey(conversation.peerPubkey)

    Row(
        modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        NostrAvatar(
            pubkey = conversation.peerPubkey,
            size = 36.dp,
            consumerId = "dm-peer-${conversation.peerPubkey}",
        )
        Spacer(Modifier.size(8.dp))

        // Peer pubkey, timestamp, and message preview
        Column(
            Modifier
                .weight(1f)
                .fillMaxWidth()
        ) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    peerShortHex,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                    modifier = Modifier.weight(1f),
                )
                if (latest != null) {
                    Spacer(Modifier.size(4.dp))
                    Text(
                        formatRelativeTime(latest.createdAt),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (latest != null) {
                Text(
                    latest.content,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/**
 * A single DM conversation thread — displays messages and a compose row.
 */
@Composable
private fun DmConversationView(
    model: KernelModel,
    peerPubkey: String,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // ADR-0063 Lane G: the open DM conversation is a live profile surface for the
    // peer — resolve the full ProfileCard shape with Live liveness (tailing sub).
    DisposableEffect(peerPubkey) {
        model.resolveProfile(peerPubkey, "dm-thread", RefShape.ProfileCard, RefLiveness.Live)
        onDispose { model.releaseProfile(peerPubkey, "dm-thread") }
    }

    val s by model.state.collectAsStateWithLifecycle()
    val dmInbox = s.projections?.dmInbox ?: DmInboxSnapshot()
    val conversation = dmInbox.conversations.firstOrNull { it.peerPubkey == peerPubkey }

    var draftMessage by remember { mutableStateOf("") }

    val peerShortHex = shortPubkey(peerPubkey)

    Column(modifier.fillMaxSize()) {
        // Header
        Row(
            Modifier
                .fillMaxWidth()
                .padding(12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(onClick = onBack, Modifier.padding(end = 8.dp)) {
                Text("Back")
            }
            Text(
                peerShortHex,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
            Spacer(Modifier.weight(1f))
        }
        HorizontalDivider()

        // Message stream
        if (conversation?.messages.isNullOrEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    "No messages yet\nSend a private NIP-17 message to start the conversation.",
                    textAlign = TextAlign.Center,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(24.dp),
                )
            }
        } else {
            LazyColumn(
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(vertical = 8.dp)
            ) {
                itemsIndexed(
                    conversation!!.messages,
                    key = { _, msg -> msg.id }
                ) { _, message ->
                    DmMessageBubble(message = message)
                }
            }
        }

        // Compose row
        HorizontalDivider()
        Row(
            Modifier
                .fillMaxWidth()
                .padding(8.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TextField(
                value = draftMessage,
                onValueChange = { draftMessage = it },
                label = { Text("Message…") },
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(8.dp)),
                maxLines = 3,
                colors = TextFieldDefaults.colors(
                    unfocusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                    focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                ),
            )
            IconButton(
                onClick = {
                    val trimmed = draftMessage.trim()
                    if (trimmed.isNotEmpty()) {
                        // Fire-and-forget: dispatch nmp.nip17.send action.
                        // The sent message reappears through the next snapshot tick
                        // (the actor gift-wraps a self-copy to the sender).
                        model.sendDm(peerPubkey, trimmed)
                        draftMessage = ""
                    }
                },
                enabled = draftMessage.trim().isNotEmpty(),
            ) {
                Icon(Icons.Filled.Send, contentDescription = "Send")
            }
        }
    }
}

/**
 * A single DM message bubble. Outgoing messages align right; incoming align left.
 */
@Composable
private fun DmMessageBubble(message: DmMessage) {
    val outgoing = message.isOutgoing
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp, horizontal = 12.dp),
    ) {
        if (outgoing) Spacer(Modifier.weight(1f))
        Column(
            horizontalAlignment = if (outgoing) Alignment.End else Alignment.Start,
            modifier = Modifier
                .weight(1f)
                .padding(horizontal = 8.dp),
        ) {
            Surface(
                color = if (outgoing) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.surfaceVariant
                },
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier.padding(vertical = 2.dp),
            ) {
                Text(
                    message.content,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (outgoing) {
                        MaterialTheme.colorScheme.onPrimary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
            Text(
                formatRelativeTime(message.createdAt),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
        if (!outgoing) Spacer(Modifier.weight(1f))
    }
}

/**
 * Empty state: no DM conversations yet.
 */
@Composable
private fun EmptyDmState() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "No chats yet",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.size(8.dp))
            Text(
                "Your chats are private and end-to-end encrypted.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

/**
 * ADR-0050 §D7 "unavailable" state: no active account — hide the DM screen.
 */
@Composable
private fun UnavailableDmState() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "DMs unavailable",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.size(8.dp))
            Text(
                "Sign in to an account to send and read end-to-end encrypted messages.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

