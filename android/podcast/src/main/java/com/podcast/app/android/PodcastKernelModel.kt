package com.podcast.app.android

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.podcast.app.android.bridge.FeedFetcher
import com.podcast.app.android.bridge.PodcastKernelBridge
import com.podcast.app.android.model.PodcastFeedView
import com.podcast.app.android.model.PodcastLibraryView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

private const val TAG = "NmpPodcast"

/**
 * Observable mirror of the kernel snapshot for the podcast app — Android peer
 * of the iOS `PodcastBridge.swift`. Pure mirror: every UI mutation is driven
 * by a kernel snapshot frame or a direct podcast snapshot refresh; the Kotlin
 * side has zero business logic or derived state (D5/D8). Decode fails closed
 * (D1).
 *
 * **T-podcast-android-4** — host-side OkHttp fetch wired (Option A from
 * T-podcast-gap-3). After a successful subscribe, [onAddPodcastPressed] launches
 * a background coroutine via [FeedFetcher] that:
 *   1. Downloads the raw RSS/Atom bytes from the feed URL.
 *   2. Passes them to [bridge.ingestBytes] → Rust parser → episode table.
 *   3. Re-fetches the library snapshot so `episodeCount` is updated.
 *   4. If [selectedPodcastId] is set to the new podcast, re-fetches episodes too.
 *
 * Network or parse failures surface as toast events (D6 — never silent).
 * The episode list is empty until the fetch completes — correct and honest.
 *
 * Prior note (T-podcast-android-3): "HTTP-fetch gap: host-side OkHttp fetch
 * NOT called on subscribe yet (T-podcast-gap-3)" — resolved in this iteration.
 */
class PodcastKernelModel : ViewModel() {

    private val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
        @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
        namingStrategy = kotlinx.serialization.json.JsonNamingStrategy.SnakeCase
    }

    private val bridge = PodcastKernelBridge()
    private val fetcher = FeedFetcher()

    private val _library = MutableStateFlow(PodcastLibraryView())
    val library: StateFlow<PodcastLibraryView> = _library.asStateFlow()

    private val _episodes = MutableStateFlow(PodcastFeedView())
    val episodes: StateFlow<PodcastFeedView> = _episodes.asStateFlow()

    /** The podcast id whose episodes are currently shown (null = library view). */
    private val _selectedPodcastId = MutableStateFlow<String?>(null)
    val selectedPodcastId: StateFlow<String?> = _selectedPodcastId.asStateFlow()

    private val _snapshotCount = MutableStateFlow(0L)
    val snapshotCount: StateFlow<Long> = _snapshotCount.asStateFlow()

    /** One-shot events surfaced to the UI (e.g. "Already subscribed" toast). */
    private val _toastEvent = MutableSharedFlow<String>(extraBufferCapacity = 4)
    val toastEvent: SharedFlow<String> = _toastEvent.asSharedFlow()

    private var started = false

    fun start() {
        if (started) return
        started = true
        bridge.start(visibleLimit = 80, emitHz = 4)
        refreshFromPodcastSnapshot()
        viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val payload = bridge.nextUpdate() ?: continue
                val view = decodeLibrary(payload)
                withContext(Dispatchers.Main) {
                    if (view != null) {
                        _library.value = view
                    }
                    _snapshotCount.value += 1
                }
            }
        }
    }

    /**
     * Subscribe to a podcast feed, then fetch + ingest its bytes (T-podcast-android-4).
     *
     * Subscribe path:
     *   - [bridge.subscribe] adds the feed URL to the Rust state.
     *   - On success: refresh [library] from the podcast snapshot.
     *   - On failure (dedup / invalid URL): emit a [toastEvent] (D6).
     *
     * Fetch path (runs only after a successful subscribe):
     *   - [FeedFetcher.fetchAndIngest] downloads bytes via OkHttp and calls
     *     [bridge.ingestBytes] so the Rust parser populates the episode table.
     *   - Success: refresh [library] (episodeCount now reflects parsed episodes).
     *     If the newly added podcast is currently selected, also refresh [episodes].
     *   - Failure: emit a descriptive [toastEvent] (D6 — never silent).
     */
    fun onAddPodcastPressed(feedUrl: String, title: String? = null, author: String? = null) {
        viewModelScope.launch(Dispatchers.IO) {
            Log.i(TAG, "onAddPodcastPressed: feedUrl=$feedUrl")
            val added = bridge.subscribe(feedUrl, title, author)
            if (!added) {
                Log.w(TAG, "subscribe returned false — dedup or invalid URL for: $feedUrl")
                withContext(Dispatchers.Main) {
                    _toastEvent.tryEmit("Could not add podcast. Check the feed URL.")
                }
                return@launch
            }

            Log.i(TAG, "subscribe succeeded — refreshing library then fetching feed bytes")
            // Refresh library so the new podcast row is visible immediately
            // (episodeCount = 0 is honest until the fetch completes).
            val snapAfterSubscribe = bridge.snapshot()
            if (snapAfterSubscribe != null) {
                val view = decodePodcastSnapshot(snapAfterSubscribe)
                withContext(Dispatchers.Main) {
                    if (view != null) _library.value = view
                }
            }

            // Host-side fetch: download feed bytes and ingest into Rust state.
            val fetchResult = fetcher.fetchAndIngest(bridge, feedUrl)
            when (fetchResult) {
                is FeedFetcher.FetchResult.Success -> {
                    Log.i(TAG, "fetchAndIngest OK — ${fetchResult.episodeCount} episodes")
                    // Re-read library so episodeCount is updated in the podcast row.
                    val snapAfterIngest = bridge.snapshot()
                    val updatedLibrary = snapAfterIngest?.let { decodePodcastSnapshot(it) }
                    // If this podcast is currently selected, refresh the episode list.
                    val curId = _selectedPodcastId.value
                    val updatedEpisodes = if (curId != null) {
                        bridge.episodes(curId)?.let { decodeFeedView(it) }
                    } else null
                    withContext(Dispatchers.Main) {
                        if (updatedLibrary != null) _library.value = updatedLibrary
                        if (updatedEpisodes != null) _episodes.value = updatedEpisodes
                    }
                }
                is FeedFetcher.FetchResult.Failure -> {
                    Log.w(TAG, "fetchAndIngest failed: ${fetchResult.reason}")
                    withContext(Dispatchers.Main) {
                        _toastEvent.tryEmit("Feed downloaded but could not be read: ${fetchResult.reason}")
                    }
                }
            }
        }
    }

    /**
     * Unsubscribe a podcast by its ULID id. Updates the library immediately
     * from the podcast snapshot after the FFI call.
     */
    fun onUnsubscribePressed(podcastId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            Log.i(TAG, "onUnsubscribePressed: id=$podcastId")
            bridge.unsubscribe(podcastId)
            val snap = bridge.snapshot()
            if (snap != null) {
                val view = decodePodcastSnapshot(snap)
                withContext(Dispatchers.Main) {
                    if (view != null) _library.value = view
                    // Clear selection if the selected podcast was removed.
                    if (_selectedPodcastId.value == podcastId) {
                        _selectedPodcastId.value = null
                        _episodes.value = PodcastFeedView()
                    }
                }
            }
        }
    }

    /**
     * Navigate to the episode list for a podcast. Fetches episodes from the
     * Rust state via [bridge.episodes]. Empty list renders an honest empty
     * state (not an error) — T-podcast-gap-3 tracks why episodes may be empty.
     */
    fun onPodcastSelected(podcastId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            Log.i(TAG, "onPodcastSelected: id=$podcastId")
            _selectedPodcastId.value = podcastId
            val raw = bridge.episodes(podcastId)
            val view = if (raw != null) decodeFeedView(raw) else PodcastFeedView()
            withContext(Dispatchers.Main) {
                _episodes.value = view ?: PodcastFeedView()
            }
        }
    }

    /**
     * Navigate back from the episode list to the library.
     */
    fun onBackFromEpisodes() {
        _selectedPodcastId.value = null
        _episodes.value = PodcastFeedView()
    }

    // --- Decode helpers -------------------------------------------------------

    /**
     * Decode one frame from the kernel `update_tx` channel.
     * Pre-T-podcast-gap-1: the snapshot has no `library` field, so this
     * returns null often. Library is populated via [refreshFromPodcastSnapshot].
     */
    private fun decodeLibrary(payload: String): PodcastLibraryView? {
        val outer = runCatching {
            json.parseToJsonElement(payload).jsonObject
        }.getOrElse { e ->
            Log.e(TAG, "envelope parse failed: ${e.message}; prefix: ${payload.take(200)}")
            return null
        }
        val tag = outer["t"]?.jsonPrimitive?.content
        if (tag != "snapshot") {
            if (tag == "update") {
                Log.d(TAG, "discrete update frame received (ignored by snapshot model)")
            } else {
                Log.e(TAG, "unknown envelope tag=$tag; prefix: ${payload.take(200)}")
            }
            return null
        }
        val inner = outer["v"]?.jsonObject ?: run {
            Log.e(TAG, "snapshot envelope missing 'v' field")
            return null
        }
        val libElem = inner["library"]?.jsonObject ?: return null  // T-podcast-gap-1
        return runCatching {
            json.decodeFromJsonElement<PodcastLibraryView>(libElem)
        }.getOrElse { e ->
            Log.e(TAG, "PodcastLibraryView decode failed: ${e.message}")
            null
        }
    }

    /**
     * Decode a raw podcast snapshot JSON string from [PodcastKernelBridge.snapshot].
     * Shape: `{"podcasts":[…]}` — the `LibraryView` from `nmp_app_podcast_snapshot`.
     */
    private fun decodePodcastSnapshot(json_str: String): PodcastLibraryView? {
        return runCatching {
            json.decodeFromString<PodcastLibraryView>(json_str)
        }.getOrElse { e ->
            Log.e(TAG, "decodePodcastSnapshot failed: ${e.message}; prefix: ${json_str.take(200)}")
            null
        }
    }

    /**
     * Decode a raw `FeedView` JSON string from [PodcastKernelBridge.episodes].
     * Shape: `{"episodes":[…]}`.
     */
    private fun decodeFeedView(json_str: String): PodcastFeedView? {
        return runCatching {
            json.decodeFromString<PodcastFeedView>(json_str)
        }.getOrElse { e ->
            Log.e(TAG, "decodeFeedView failed: ${e.message}; prefix: ${json_str.take(200)}")
            null
        }
    }

    /** Pull the podcast snapshot immediately and update [library]. */
    private fun refreshFromPodcastSnapshot() {
        viewModelScope.launch(Dispatchers.IO) {
            val snap = bridge.snapshot() ?: return@launch
            val view = decodePodcastSnapshot(snap) ?: return@launch
            withContext(Dispatchers.Main) {
                _library.value = view
            }
        }
    }

    override fun onCleared() {
        bridge.free()
        super.onCleared()
    }
}
