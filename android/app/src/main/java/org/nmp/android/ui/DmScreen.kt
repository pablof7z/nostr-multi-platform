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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Send
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.nmp.android.KernelModel
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
    val s by model.state.collectAsStateWithLifecycle()
    val dmInbox = s.projections?.dmInbox ?: DmInboxSnapshot()

    var selectedPeerPubkey by remember { mutableStateOf<String?>(null) }

    Box(modifier.fillMaxSize()) {
        if (selectedPeerPubkey != null) {
            DmConversationView(
                model = model,
                peerPubkey = selectedPeerPubkey!!,
                onBack = { selectedPeerPubkey = null }
            )
        } else {
            DmConversationListScreen(
                dmInbox = dmInbox,
                onSelectConversation = { pubkey -> selectedPeerPubkey = pubkey }
            )
        }
    }
}

/**
 * The conversation list (newest-thread-first).
 */
@Composable
private fun DmConversationListScreen(
    dmInbox: DmInboxSnapshot,
    onSelectConversation: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Chats", style = MaterialTheme.typography.headlineSmall)
        }
        HorizontalDivider()

        if (dmInbox.remoteSignerUnsupported) {
            BunkerUnsupportedState()
        } else if (dmInbox.conversations.isEmpty()) {
            EmptyDmState()
        } else {
            ConversationListContent(
                conversations = dmInbox.conversations,
                onSelectConversation = onSelectConversation
            )
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
) {
    LazyColumn(modifier.fillMaxSize()) {
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
 * One row in the DM conversation list.
 * Displays the peer's pubkey (shortened), last message preview, and timestamp.
 */
@Composable
private fun DmConversationRow(
    conversation: DmConversation,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val latest = conversation.messages.lastOrNull()
    val peerShortHex = if (conversation.peerPubkey.length >= 16) {
        "${conversation.peerPubkey.take(8)}…${conversation.peerPubkey.takeLast(8)}"
    } else {
        conversation.peerPubkey
    }

    Row(
        modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Avatar (initials only — per ADR-0032, derived from pubkey)
        Avatar(peerShortHex.take(2).uppercase(), "")
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
    val s by model.state.collectAsStateWithLifecycle()
    val dmInbox = s.projections?.dmInbox ?: DmInboxSnapshot()
    val conversation = dmInbox.conversations.firstOrNull { it.peerPubkey == peerPubkey }

    var draftMessage by remember { mutableStateOf("") }

    val peerShortHex = if (peerPubkey.length >= 16) {
        "${peerPubkey.take(8)}…${peerPubkey.takeLast(8)}"
    } else {
        peerPubkey
    }

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
                        val actionJson = """{"recipient_pubkey":"$peerPubkey","content":"${escapeJson(trimmed)}"}"""
                        model.dispatchAction("nmp.nip17.send", actionJson)
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
 * Avatar with initials (no color differentiation in this minimal version).
 */
@Composable
private fun Avatar(initials: String, colorHex: String) {
    Surface(
        modifier = Modifier.size(36.dp).clip(CircleShape),
        color = parseHexColor(colorHex) ?: MaterialTheme.colorScheme.secondary,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(
                initials,
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
        }
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
 * Bunker (NIP-46) unsupported state: cannot decrypt gift-wraps.
 */
@Composable
private fun BunkerUnsupportedState() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "DMs unavailable",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.size(8.dp))
            Text(
                "End-to-end encrypted DMs require a local key.\nBunker (NIP-46) accounts cannot decrypt messages yet.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

/**
 * Format Unix seconds as relative time (e.g., "5m ago", "2h ago").
 * Mirrors the iOS presentation-layer concern (D8).
 */
private fun formatRelativeTime(createdAtSeconds: Long): String {
    val deltaSecs = (System.currentTimeMillis() / 1000) - createdAtSeconds
    return when {
        deltaSecs < 60 -> "${deltaSecs}s ago"
        deltaSecs < 3_600 -> "${deltaSecs / 60}m ago"
        deltaSecs < 86_400 -> "${deltaSecs / 3_600}h ago"
        else -> "${deltaSecs / 86_400}d ago"
    }
}

/**
 * Escape special JSON characters in a string.
 * Mirrors [KernelModel.escapeJson].
 */
private fun escapeJson(s: String): String {
    return s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
}

/**
 * Parse a hex color string (e.g. "#RRGGBB" or "RRGGBB") into a Compose [Color].
 * Returns null if the string is blank or malformed.
 */
private fun parseHexColor(hex: String): Color? {
    if (hex.isBlank()) return null
    return try {
        val cleaned = if (hex.startsWith("#")) hex.substring(1) else hex
        val value = cleaned.toLong(16)
        val argb = if (cleaned.length == 6) (0xFF000000L or value).toInt() else value.toInt()
        Color(argb)
    } catch (_: NumberFormatException) {
        null
    }
}
