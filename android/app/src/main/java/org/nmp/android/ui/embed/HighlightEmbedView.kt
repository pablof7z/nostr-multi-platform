package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.unit.dp
import org.nmp.android.model.HighlightProjectionEntry

/**
 * kind:9802 (NIP-84) highlight embed (#984). Android peer of iOS
 * `DefaultHighlightRenderer`. Renders the highlighted text as a pull-quote
 * plus the resolved byline; the optional source link is shown verbatim.
 */
@Composable
fun HighlightEmbedView(highlight: HighlightProjectionEntry) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EmbedByline(
            authorPubkey = highlight.authorPubkey,
            authorDisplayName = highlight.authorDisplayName,
            caption = "highlight",
            avatarConsumerId = "embed-highlight-${highlight.id}",
        )
        Text(
            "“${highlight.highlightedText}”",
            style = MaterialTheme.typography.bodyLarge.copy(fontStyle = FontStyle.Italic),
            modifier = Modifier.padding(start = 6.dp),
        )
        highlight.sourceUrl?.takeIf { it.isNotEmpty() }?.let { url ->
            Text(
                url,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}
