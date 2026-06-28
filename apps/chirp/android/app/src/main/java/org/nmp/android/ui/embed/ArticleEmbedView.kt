package org.nmp.android.ui.embed

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.nmp.android.model.ArticleProjectionEntry
import org.nmp.android.ui.RemoteImage

/**
 * kind:30023 long-form article embed (#984). Android peer of iOS
 * `DefaultArticleRenderer`. Paints the hero image + title + summary + byline —
 * every field is already resolved by the kernel's embed resolver (title /
 * summary / hero come from the NIP-23 tags Rust-side, not parsed here).
 */
@Composable
fun ArticleEmbedView(article: ArticleProjectionEntry) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        article.heroImageUrl?.takeIf { it.isNotEmpty() }?.let { RemoteImage(it) }
        EmbedByline(
            authorPubkey = article.authorPubkey,
            authorDisplayName = article.authorDisplayName,
            caption = "article",
            avatarConsumerId = "embed-article-${article.id}",
        )
        article.title?.takeIf { it.isNotEmpty() }?.let { title ->
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
        }
        val body = article.summary?.takeIf { it.isNotEmpty() }
            ?: article.content.takeIf { it.isNotEmpty() }
        body?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
