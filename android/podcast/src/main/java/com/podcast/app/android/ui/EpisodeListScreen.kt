package com.podcast.app.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.podcast.app.android.PodcastKernelModel
import com.podcast.app.android.model.EpisodeRowPayload

/**
 * Episode list screen — shows episodes for a single podcast.
 *
 * T-podcast-android-3: native Compose polish per the forcing-function policy
 * (no reference Android app exists). Data comes exclusively from the Rust
 * snapshot via [PodcastKernelModel.episodes] — no Kotlin-side fabrication.
 *
 * T-podcast-android-5: episode rows are now tappable. Tapping navigates to
 * [EpisodeDetailScreen] for that episode via [onEpisodeSelected].
 *
 * T-podcast-android-6: pull-to-refresh wired. Dragging down re-fetches the
 * feed bytes via [FeedFetcher] (same OkHttp path as subscribe) and re-ingests
 * into Rust state. [isRefreshing] drives the spinner — no fake delay, real
 * fetch. A 404 or parse error surfaces as a toast (D6 — never silent).
 *
 * Feed URL for the re-fetch comes from [PodcastRowPayload.feedUrl] — projected
 * by Rust in T-podcast-android-6 so the host never maintains a separate index.
 *
 * Doctrine compliance:
 *   - D5: no business logic — all state lives in Rust.
 *   - D6: empty state, refresh spinner, and error toasts are all honest.
 *   - D8: verbatim mirror of the Rust-emitted FeedView snapshot.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EpisodeListScreen(
    podcastId: String,
    model: PodcastKernelModel,
    onBack: () -> Unit,
    onEpisodeSelected: (EpisodeRowPayload) -> Unit = {},
    modifier: Modifier = Modifier,
) {
    // Trigger episode fetch whenever this screen is first shown for a podcast.
    LaunchedEffect(podcastId) {
        model.onPodcastSelected(podcastId)
    }

    val feedView by model.episodes.collectAsStateWithLifecycle()
    val library by model.library.collectAsStateWithLifecycle()
    val isRefreshing by model.isRefreshing.collectAsStateWithLifecycle()

    // Resolve the podcast row — needed for title + feed_url (for refresh).
    val podcastRow = library.podcasts.firstOrNull { it.id == podcastId }
    val podcastTitle = podcastRow?.title ?: "Episodes"
    val feedUrl = podcastRow?.feedUrl ?: ""

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = podcastTitle,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back to library",
                        )
                    }
                },
            )
        },
    ) { inner ->
        // PullToRefreshBox (Material3 1.3.0+): isRefreshing controls the indicator;
        // onRefresh is called when the user completes the pull gesture (D6 — real
        // network fetch, never a spinner with fake delay).
        PullToRefreshBox(
            isRefreshing = isRefreshing,
            onRefresh = { model.onRefreshPodcast(podcastId, feedUrl) },
            modifier = Modifier
                .fillMaxSize()
                .padding(inner),
            contentAlignment = Alignment.TopCenter,
        ) {
            if (feedView.episodes.isEmpty()) {
                EpisodeEmptyState(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(24.dp),
                )
            } else {
                LazyColumn(modifier = Modifier.fillMaxSize()) {
                    items(feedView.episodes, key = { it.id }) { episode ->
                        EpisodeRow(
                            episode = episode,
                            onClick = { onEpisodeSelected(episode) },
                        )
                        HorizontalDivider(
                            modifier = Modifier.padding(horizontal = 16.dp),
                            thickness = 0.5.dp,
                            color = MaterialTheme.colorScheme.outlineVariant,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun EpisodeEmptyState(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = "No episodes yet",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = "Episodes appear here once the feed is refreshed.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun EpisodeRow(episode: EpisodeRowPayload, onClick: () -> Unit = {}) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(
            text = episode.title,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.Medium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            modifier = Modifier.padding(top = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (episode.pubDateStr.isNotEmpty()) {
                Text(
                    text = episode.pubDateStr,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (episode.durationStr.isNotEmpty()) {
                Text(
                    text = episode.durationStr,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (episode.downloadState.isNotEmpty()) {
                Text(
                    text = episode.downloadState.replace("NotDownloaded", "Not downloaded"),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (!episode.summary.isNullOrEmpty()) {
            Text(
                text = episode.summary,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }
}
