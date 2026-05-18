package com.podcast.app.android.bridge

import org.nmp.android.KernelBridge

/**
 * NmpPodcast wrapper around the kernel JNI bridge — T157 step 2.
 *
 * Delegates to [org.nmp.android.KernelBridge] (re-paste of NmpPulse's
 * bridge under the same package — see that class's KDoc for the JNI
 * symbol-naming rationale). The wrapper layer exists so the podcast app
 * can grow podcast-specific bridge methods (`subscribePodcast`,
 * `librarySnapshot`, …) once the FFI surface is built, without forcing
 * NmpPulse to grow podcast-shaped methods on its bridge.
 *
 * **Functional state today (T157 step 2):**
 *   - Kernel lifecycle: ✅ wired (this delegates start/stop/nextUpdate/free)
 *   - Snapshot stream: ✅ flows through (decoded in
 *     `com.podcast.app.android.PodcastKernelModel`)
 *   - Podcast-specific FFI (e.g. `nmp_podcast_subscribe`,
 *     `nmp_podcast_library`): ❌ blocked on **T-podcast-gap-2** — no
 *     `nmp_podcast_*` FFI surface exists; iOS hasn't built one either.
 *     When that lands, this wrapper grows `nativeSubscribePodcast` /
 *     `nativeLibrarySnapshot`, mirroring what iOS adds to its bridge.
 *
 * Doctrine: pure transport (D5). No business logic, no cached state, no
 * derived view models. Errors never cross FFI (D6) — the snapshot stream
 * carries outcomes.
 */
class PodcastKernelBridge {

    private val inner = KernelBridge()

    /**
     * Start the kernel. `visibleLimit` / `emitHz` are kernel knobs documented
     * in `docs/ffi-surface.md`; the podcast app picks the same defaults as
     * NmpPulse for parity.
     */
    fun start(visibleLimit: Int = 80, emitHz: Int = 4) {
        inner.start(visibleLimit, emitHz)
    }

    fun stop() = inner.stop()

    /**
     * Blocking (≤250 ms) drain of the kernel snapshot channel. Returns the raw
     * JSON envelope (or null on idle / shutdown). The podcast app's
     * `PodcastKernelModel` decodes it via the same `{"t":"snapshot","v":{…}}`
     * envelope contract used by Chirp / NmpPulse (T103 / T107).
     *
     * Today the snapshot has no `podcasts`/`library` field — `podcast-core`
     * hasn't registered a `LibraryViewModule` yet (T-podcast-gap-1). The
     * decoded `PodcastLibraryView` therefore stays at its default empty
     * value, and the UI renders the canonical 'No Podcasts' empty state.
     */
    fun nextUpdate(): String? = inner.nextUpdate()

    fun free() = inner.free()
}
