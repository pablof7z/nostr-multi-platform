package com.podcast.app.android

import android.util.Log
import androidx.lifecycle.ViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import com.podcast.app.android.model.PodcastLibraryView

private const val TAG = "NmpPodcast"

/**
 * Observable mirror of the kernel snapshot for the podcast app — the Android
 * peer of (future) iOS `PodcastBridge.swift`.
 *
 * **T157 step 1 (THIS COMMIT)** — the ViewModel is scaffolded with the
 * StateFlow contract the UI consumes, but the JNI bridge wiring lives in step
 * 2 (commit `feat(android-podcast/jni): library JNI bridge`). For now
 * [library] stays at the default empty value, which is the **honest
 * production state**: no podcasts are subscribed and no kernel view module is
 * producing rows yet. The "No Podcasts" empty state renders correctly.
 *
 * **Gaps surfaced by this scaffold:**
 *   - `T-podcast-gap-1` — `podcast-core` has no kernel integration (no view
 *     registry hookup, no `LibraryViewModule` registered, no action
 *     dispatcher). See `docs/perf/m11/T-podcast-gap-1.md`.
 *   - `T-podcast-gap-2` — no `nmp_podcast_*` FFI surface exists; iOS hasn't
 *     built one either. See `docs/perf/m11/T-podcast-gap-2.md`.
 *
 * Doctrine: this class is a pure mirror (D8) of the kernel snapshot. No
 * Kotlin-side filter / sort / dedup; failures fail-closed (D1) and never
 * cross FFI (D6).
 */
class PodcastKernelModel : ViewModel() {

    private val _library = MutableStateFlow(PodcastLibraryView())
    val library: StateFlow<PodcastLibraryView> = _library.asStateFlow()

    private var started = false

    fun start() {
        if (started) return
        started = true
        Log.i(
            TAG,
            "PodcastKernelModel.start — JNI bridge wiring lands in T157 step 2; " +
                "Library will stay empty until T-podcast-gap-1/2 close.",
        )
    }

    /**
     * Add-podcast CTA. Stub-with-TODO: dispatch lands when
     * `PodcastAction::SubscribePodcast` is reachable via FFI (T-podcast-gap-2).
     * Today the action enum exists in `apps/podcast/podcast-core/src/actions/`
     * but is not wired to the kernel actor — pressing the button logs a marker
     * so QA can see the call path is reachable end-to-end.
     */
    fun onAddPodcastPressed() {
        Log.i(TAG, "onAddPodcastPressed — dispatch blocked on T-podcast-gap-2")
    }

    override fun onCleared() {
        super.onCleared()
    }
}
