package com.podcast.app.android.model

import kotlinx.serialization.Serializable

/**
 * Verbatim Kotlin mirror of `podcast_core::views::EpisodeRowPayload`
 * (see `apps/podcast/podcast-core/src/views/mod.rs`). One row in the
 * episode list for a single podcast.
 *
 * Doctrine: defaulted nullable fields so an older / trimmed kernel snapshot
 * still decodes and the model keeps its prior value (D1). camelCase Kotlin
 * ↔ snake_case JSON via the parent `Json { namingStrategy = SnakeCase }`.
 * No derived state — D8 verbatim mirror.
 */
@Serializable
data class EpisodeRowPayload(
    val id: String = "",
    val title: String = "",
    val podcastTitle: String = "",
    val podcastArtworkUrl: String? = null,
    val summary: String? = null,
    val durationStr: String = "",
    /** Human-readable publication date, e.g. "Jan 1, 2024". Empty when feed omitted pubDate. */
    val pubDateStr: String = "",
    val downloadState: String = "",
    val activeJobKind: String? = null,
    val hasInsights: Boolean = false,
    val insightsCount: Long = 0,
    val isPlaying: Boolean = false,
)

/**
 * Verbatim Kotlin mirror of `podcast_core::views::FeedView`. Wraps the
 * episode row list returned by `nmp_app_podcast_episodes`.
 */
@Serializable
data class PodcastFeedView(
    val episodes: List<EpisodeRowPayload> = emptyList(),
)
