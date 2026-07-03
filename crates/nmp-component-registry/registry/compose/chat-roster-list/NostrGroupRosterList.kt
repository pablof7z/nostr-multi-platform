package org.nmp.registry

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

@Composable
fun NostrGroupRosterList(
    participants: List<NostrGroupChatParticipantWire>,
    modifier: Modifier = Modifier,
    onParticipantClick: (String) -> Unit = {},
) {
    Column(modifier = modifier) {
        participants.forEach { participant ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onParticipantClick(participant.pubkey) }
                    .padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                NostrAvatar(
                    pubkey = participant.pubkey,
                    size = 36.dp,
                    consumerId = "chat.roster.${participant.pubkey}.avatar",
                )
                Spacer(Modifier.width(10.dp))
                Column(modifier = Modifier.weight(1f)) {
                    NostrProfileName(
                        pubkey = participant.pubkey,
                        style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold),
                        consumerId = "chat.roster.${participant.pubkey}.name",
                    )
                    val metadata = listOfNotNull(
                        participant.roleLabel?.takeIf { it.isNotBlank() },
                        participant.statusLabel?.takeIf { it.isNotBlank() },
                    ).joinToString("   ")
                    if (metadata.isNotEmpty()) {
                        Text(
                            text = metadata,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}
