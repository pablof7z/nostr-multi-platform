package com.podcast.app.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.LibraryBooks
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.podcast.app.android.PodcastKernelModel
import com.podcast.app.android.model.PodcastRowPayload

/**
 * Library tab — Android parity of `ios/NmpPodcast/.../Views/Library/LibraryView.swift`.
 *
 * Doctrine compliance:
 *   - **D8 verbatim mirror**: reads `LibraryView { podcasts: [...] }` straight
 *     from the kernel-emitted snapshot. No Kotlin-side sort / filter / dedup
 *     (the iOS Swift view does `@Query(sort: \Podcast.title)` against
 *     SwiftData; the kernel is responsible for the equivalent ordering when
 *     `podcast-core` wires its `LibraryViewModule` — see T-podcast-gap-1).
 *   - **D5 no business logic**: the "Add podcast" CTA is a stub-with-TODO;
 *     dispatch lands when `PodcastAction::SubscribePodcast` is reachable via
 *     FFI (see T-podcast-gap-2).
 *
 * Visual fidelity is approximate, not pixel-perfect — that gate only applies
 * to iOS Swift parity per `docs/design/podcast-app-rebuild.md` §1.
 * Android shell is functional parity only.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    model: PodcastKernelModel,
    modifier: Modifier = Modifier,
) {
    val library by model.library.collectAsStateWithLifecycle()
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(title = { Text("Library") })
        },
    ) { inner ->
        if (library.podcasts.isEmpty()) {
            LibraryEmptyState(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(inner)
                    .padding(24.dp),
                onAddPodcast = model::onAddPodcastPressed,
            )
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(inner),
            ) {
                items(library.podcasts, key = { it.id }) { podcast ->
                    PodcastRow(podcast)
                }
            }
        }
    }
}

@Composable
private fun LibraryEmptyState(
    modifier: Modifier = Modifier,
    onAddPodcast: () -> Unit,
) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            imageVector = Icons.AutoMirrored.Filled.LibraryBooks,
            contentDescription = null,
            modifier = Modifier.padding(bottom = 8.dp),
        )
        Text(
            text = "No Podcasts",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = "Subscribe to podcasts to build your library.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Button(
            onClick = onAddPodcast,
            modifier = Modifier.padding(top = 8.dp),
        ) {
            Text("Add podcast")
        }
    }
}

@Composable
private fun PodcastRow(podcast: PodcastRowPayload) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(
            text = podcast.title,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.Medium,
        )
        Text(
            text = podcast.author,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = "${podcast.episodeCount} episodes",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
