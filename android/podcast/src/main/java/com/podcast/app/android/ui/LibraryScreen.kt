package com.podcast.app.android.ui

import android.widget.Toast
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.LibraryBooks
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.podcast.app.android.PodcastKernelModel
import com.podcast.app.android.model.PodcastRowPayload

/**
 * Library tab — Android parity of `ios/NmpPodcast/.../Views/Library/LibraryView.swift`.
 *
 * T-podcast-android-2: "Add podcast" CTA now opens a dialog where the user
 * enters a feed URL. The dialog dispatches [PodcastKernelModel.onAddPodcastPressed]
 * which calls `nmp_app_podcast_subscribe` via JNI and refreshes the library
 * from the podcast snapshot (D8 verbatim mirror).
 *
 * Doctrine compliance:
 *   - D5 no business logic: URL validation lives in Rust (`url::Url::parse`).
 *   - D6 error surface: `onAddPodcastPressed` emits a [toastEvent] on failure;
 *     this screen collects it and shows an Android Toast.
 *   - D8 verbatim mirror: the list renders the Rust-emitted snapshot directly;
 *     no Kotlin-side sort, filter, or dedup.
 *
 * Visual fidelity is approximate (functional parity only, not pixel-perfect).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(
    model: PodcastKernelModel,
    modifier: Modifier = Modifier,
) {
    val library by model.library.collectAsStateWithLifecycle()
    var showAddDialog by remember { mutableStateOf(false) }

    val context = LocalContext.current
    LaunchedEffect(Unit) {
        model.toastEvent.collect { message ->
            Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
        }
    }

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
                onAddPodcast = { showAddDialog = true },
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

    if (showAddDialog) {
        AddPodcastDialog(
            onDismiss = { showAddDialog = false },
            onConfirm = { feedUrl ->
                showAddDialog = false
                model.onAddPodcastPressed(feedUrl)
            },
        )
    }
}

@Composable
private fun AddPodcastDialog(
    onDismiss: () -> Unit,
    onConfirm: (feedUrl: String) -> Unit,
) {
    var feedUrl by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Podcast") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    text = "Enter the RSS feed URL for the podcast you want to subscribe to.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = feedUrl,
                    onValueChange = { feedUrl = it },
                    label = { Text("Feed URL") },
                    placeholder = { Text("https://feeds.example.com/podcast.xml") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            Button(
                onClick = { if (feedUrl.isNotBlank()) onConfirm(feedUrl.trim()) },
                enabled = feedUrl.isNotBlank(),
            ) {
                Text("Subscribe")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        },
    )
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
