package com.podcast.app.android

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.podcast.app.android.bridge.PodcastKernelBridge
import com.podcast.app.android.model.PodcastLibraryView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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
 * of (future) iOS `PodcastBridge.swift`. Pure mirror: every UI mutation is
 * driven by a kernel snapshot frame; the Kotlin side has zero business logic
 * or derived state (D5/D8). Decode fails closed (D1).
 *
 * **T157 step 2 (THIS COMMIT)** — JNI bridge is wired. The kernel boots, the
 * snapshot stream flows in, and frames are decoded under the same
 * `{"t":"snapshot","v":{…}}` envelope contract used by Chirp/NmpPulse
 * (T103/T107). Because `podcast-core` has not yet registered a
 * `LibraryViewModule` with the kernel (T-podcast-gap-1), the snapshot has no
 * `library`/`podcasts` field, and [library] stays at its default empty value.
 * This is the **honest production state** — no fake rows, no Kotlin-side
 * fallback data, no business logic shim.
 *
 * When T-podcast-gap-1 lands, `podcast-core::views::LibraryView` will start
 * appearing as `snapshot.library` in the envelope, decoding will populate
 * [library] automatically, and no Kotlin changes will be needed.
 *
 * Gaps surfaced by this scaffold:
 *   - `T-podcast-gap-1` — `podcast-core` has no kernel integration
 *     (no view registry hookup, no `LibraryViewModule` registered,
 *     no action dispatcher)
 *   - `T-podcast-gap-2` — no `nmp_podcast_*` FFI surface exists; iOS hasn't
 *     built one either
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

    private var started = false

    fun start() {
        if (started) return
        started = true
        bridge.start(visibleLimit = 80, emitHz = 4)
        viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val payload = bridge.nextUpdate() ?: continue
                val view = decodeLibrary(payload) ?: continue
                withContext(Dispatchers.Main) {
                    _library.value = view
                    _snapshotCount.value += 1
                }
            }
        }
    }

    /**
     * Decode one frame from the kernel `update_tx` channel. Mirrors the
     * envelope-unwrapping in `org.nmp.android.KernelModel.decodeSnapshot` —
     * non-snapshot frames (`t=update`) are logged at DEBUG and dropped (the
     * snapshot projection already carries the full UI state).
     *
     * Returns null (drop the frame) on any parse error; logs enough context
     * to diagnose the failure without flooding logcat (PD-025 finding 4 —
     * no silent swallow).
     *
     * Pre-T-podcast-gap-1: the snapshot has no `library` field, so the
     * decoded [PodcastLibraryView] is always the defaulted empty value.
     * That's an intentional pass-through — we update the StateFlow with the
     * empty view rather than dropping the frame, so the snapshot count
     * still ticks and the UI remains live.
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
        val libElem = inner["library"]?.jsonObject ?: return PodcastLibraryView()
        return runCatching {
            json.decodeFromJsonElement<PodcastLibraryView>(libElem)
        }.getOrElse { e ->
            Log.e(TAG, "PodcastLibraryView decode failed: ${e.message}")
            null
        }
    }

    /**
     * Add-podcast CTA. Stub-with-TODO: dispatch lands when
     * `PodcastAction::SubscribePodcast` is reachable via FFI (T-podcast-gap-2).
     * Today the action enum exists in `apps/podcast/podcast-core/src/actions/`
     * but is not wired to the kernel actor — pressing the button logs a
     * marker so QA can see the call path is reachable end-to-end.
     */
    fun onAddPodcastPressed() {
        Log.i(TAG, "onAddPodcastPressed — dispatch blocked on T-podcast-gap-2")
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
