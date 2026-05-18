package com.podcast.app.android

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.podcast.app.android.bridge.PodcastKernelBridge
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
 * **T-podcast-android-2 (THIS COMMIT)** — podcast FFI now wired:
 *   - [bridge] is a real [PodcastKernelBridge] with 6 native methods.
 *   - [onAddPodcastPressed] dispatches to `nmp_app_podcast_subscribe` via
 *     [bridge.subscribe]. On success (true return) the library is refreshed
 *     from the podcast snapshot directly; on failure a [toastEvent] is emitted
 *     so the UI can surface a toast (D6 — errors never cross FFI silently).
 *   - [library] is refreshed from two sources:
 *       1. The kernel snapshot envelope (`{"t":"snapshot","v":{…}}`), which
 *          may carry a `library` field once `podcast-core` registers a
 *          LibraryViewModule (T-podcast-gap-1).
 *       2. Direct podcast snapshot via [bridge.snapshot()] after each
 *          subscribe/unsubscribe — this works TODAY without gap-1.
 */
class PodcastKernelModel : ViewModel() {

    private val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
        @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
        namingStrategy = kotlinx.serialization.json.JsonNamingStrategy.SnakeCase
    }

    private val bridge = PodcastKernelBridge()

    private val _library = MutableStateFlow(PodcastLibraryView())
    val library: StateFlow<PodcastLibraryView> = _library.asStateFlow()

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
        // Attempt an immediate snapshot so the library is populated before the
        // first kernel snapshot frame arrives (podcast handle is ready now).
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
     * Subscribe to a podcast feed. Called by [LibraryScreen] after the user
     * confirms a URL in the AddPodcast dialog.
     *
     * Success path (bridge returns true):
     *   - Refresh [library] from the podcast snapshot directly.
     * Failure path (bridge returns false — dedup or invalid URL):
     *   - Emit a [toastEvent] so the UI can surface a toast (D6 — no silent
     *     swallow). We can't distinguish dedup from invalid URL here; the
     *     snapshot already carries the truth.
     */
    fun onAddPodcastPressed(feedUrl: String, title: String? = null, author: String? = null) {
        viewModelScope.launch(Dispatchers.IO) {
            Log.i(TAG, "onAddPodcastPressed: feedUrl=$feedUrl")
            val added = bridge.subscribe(feedUrl, title, author)
            if (added) {
                Log.i(TAG, "subscribe succeeded — refreshing library")
                val snap = bridge.snapshot()
                if (snap != null) {
                    val view = decodePodcastSnapshot(snap)
                    withContext(Dispatchers.Main) {
                        if (view != null) _library.value = view
                    }
                }
            } else {
                Log.w(TAG, "subscribe returned false — dedup or invalid URL for: $feedUrl")
                withContext(Dispatchers.Main) {
                    _toastEvent.tryEmit("Could not add podcast. Check the feed URL.")
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
                }
            }
        }
    }

    /**
     * Decode one frame from the kernel `update_tx` channel. Mirrors the
     * envelope-unwrapping in `org.nmp.android.KernelModel.decodeSnapshot`.
     * Non-snapshot frames are logged at DEBUG and return null (drop the frame).
     *
     * Returns null (drop the frame) on any parse error. Logs enough context
     * to diagnose without flooding logcat.
     *
     * Pre-T-podcast-gap-1: the snapshot has no `library` field, so this
     * returns null often. That's expected — library is populated via
     * [refreshFromPodcastSnapshot] and [onAddPodcastPressed] instead.
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
        val libElem = inner["library"]?.jsonObject ?: return null  // not yet wired (T-podcast-gap-1)
        return runCatching {
            json.decodeFromJsonElement<PodcastLibraryView>(libElem)
        }.getOrElse { e ->
            Log.e(TAG, "PodcastLibraryView decode failed: ${e.message}")
            null
        }
    }

    /**
     * Decode a raw podcast snapshot JSON string (from [PodcastKernelBridge.snapshot]).
     * Shape: `{"podcasts":[…]}` — the `LibraryView` produced by
     * `nmp_app_podcast::ffi::nmp_app_podcast_snapshot`.
     */
    private fun decodePodcastSnapshot(json_str: String): PodcastLibraryView? {
        return runCatching {
            json.decodeFromString<PodcastLibraryView>(json_str)
        }.getOrElse { e ->
            Log.e(TAG, "decodePodcastSnapshot failed: ${e.message}; prefix: ${json_str.take(200)}")
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
