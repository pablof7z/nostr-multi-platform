package org.nmp.registry

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

@Composable
fun NostrGroupMessageRow(
    message: NostrGroupChatMessageWire,
    modifier: Modifier = Modifier,
    onReplyTap: (String) -> Unit = {},
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = if (message.isOutgoing) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Top,
    ) {
        if (!message.isOutgoing) {
            NostrAvatar(
                pubkey = message.authorPubkey,
                size = 32.dp,
                consumerId = "chat.message.${message.id}.avatar",
            )
            Spacer(Modifier.width(8.dp))
        }

        Column(
            horizontalAlignment = if (message.isOutgoing) Alignment.End else Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(4.dp),
            modifier = Modifier.fillMaxWidth(if (message.isOutgoing) 0.82f else 0.9f),
        ) {
            if (!message.isOutgoing) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    NostrProfileName(
                        pubkey = message.authorPubkey,
                        style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        consumerId = "chat.message.${message.id}.name",
                    )
                    Text(
                        text = message.createdAtLabel,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            message.replyPreview?.takeIf { it.isNotBlank() }?.let { preview ->
                Text(
                    text = preview,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    modifier = Modifier
                        .clickable { onReplyTap(message.id) }
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.55f),
                            RoundedCornerShape(6.dp),
                        )
                        .padding(horizontal = 8.dp, vertical = 5.dp),
                )
            }

            Surface(
                color = if (message.isOutgoing) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceVariant,
                contentColor = if (message.isOutgoing) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface,
                shape = RoundedCornerShape(8.dp),
            ) {
                Text(
                    text = message.content,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
                )
            }

            if (message.reactions.isNotEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    message.reactions.forEach { reaction ->
                        Text(
                            text = "${reaction.emoji} ${reaction.count}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier
                                .background(
                                    Color.Gray.copy(alpha = 0.12f),
                                    RoundedCornerShape(percent = 50),
                                )
                                .padding(horizontal = 7.dp, vertical = 3.dp),
                        )
                    }
                }
            }
        }
    }
}
