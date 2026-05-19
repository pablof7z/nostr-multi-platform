package com.podcast.app.android.model

import kotlinx.serialization.Serializable

/**
 * Verbatim Kotlin mirror of `podcast_core::views::PodcastRowPayload` (see
 * `apps/podcast/podcast-core/src/views/mod.rs`). One row in the Library
 * snapshot list.
 *
 * Doctrine: defaulted nullable fields so an older / trimmed kernel snapshot
 * still decodes and the model keeps its prior value (D1: best-effort,
 * fail-closed). camelCase Kotlin ↔ snake_case JSON via the parent
 * `Json { namingStrategy = SnakeCase }` configured in [PodcastKernelModel].
 *
 * No derived state — D8 verbatim mirror.
 */
@Serializable
data class PodcastRowPayload(
    val id: String = "",
    val title: String = "",
    val author: String = "",
    val artworkUrl: String? = null,
    val episodeCount: Long = 0,
    /** RSS/Atom feed URL — used by pull-to-refresh to re-fetch bytes without
     *  a separate URL index on the Kotlin side. Empty on older snapshots (D1). */
    val feedUrl: String = "",
)

/**
 * Verbatim Kotlin mirror of `podcast_core::views::LibraryView`. Wraps the
 * podcast row list emitted by the kernel `LibraryViewModule` (filed as
 * T-podcast-gap-1 until that view module is registered).
 */
@Serializable
data class PodcastLibraryView(
    val podcasts: List<PodcastRowPayload> = emptyList(),
)
