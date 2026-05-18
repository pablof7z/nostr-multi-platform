package com.podcast.app.android.bridge

import android.util.Log
import org.nmp.android.KernelBridge

private const val TAG = "PodcastBridge"

/**
 * JNI bridge for the podcast-specific FFI surface — T-podcast-android-2.
 *
 * Wraps [org.nmp.android.KernelBridge] for the core kernel lifecycle (start /
 * stop / nextUpdate / free) and owns a separate `podcastHandle` (the opaque
 * `*mut PodcastHandle` from `nmp_app_podcast_register`) for the six podcast
 * symbols.
 *
 * Why two handles?
 *   The Rust FFI has two independent object lifetimes: a `Session` (kernel
 *   snapshot pump, owned by [KernelBridge]) and a `PodcastHandle` (podcast
 *   library state, owned here). iOS holds both as separate Swift properties;
 *   we mirror that pattern. The Kotlin side MUST call [unregister] before
 *   [free] — same ordering rule as iOS `unregisterPodcast()` before
 *   `KernelHandle.free()`.
 *
 * Doctrine: pure transport (D5). No business logic, no cached state, no
 * derived view models. Errors never cross FFI (D6) — the snapshot stream
 * carries outcomes; `subscribe` returns `Boolean` to let the UI show a toast.
 */
class PodcastKernelBridge {

    private val inner = KernelBridge()

    /** Opaque `*mut PodcastHandle` as jlong; 0 == unregistered / failed. */
    private var podcastHandle: Long = 0L

    companion object {
        init {
            // The library is already loaded by KernelBridge's companion object
            // (same .so), but System.loadLibrary is idempotent — safe to repeat.
            System.loadLibrary("nmp_android_ffi")
        }
    }

    // -----------------------------------------------------------------------
    // Kernel lifecycle — delegated to KernelBridge
    // -----------------------------------------------------------------------

    /**
     * Start the kernel and register the podcast projection. Must be called
     * once before any podcast operations. `visibleLimit` / `emitHz` are
     * kernel knobs from `docs/ffi-surface.md`.
     */
    fun start(visibleLimit: Int = 80, emitHz: Int = 4) {
        inner.start(visibleLimit, emitHz)
        val kernelHandle = inner.rawHandle()
        if (kernelHandle != 0L) {
            podcastHandle = nativeRegister(kernelHandle)
            if (podcastHandle == 0L) {
                Log.e(TAG, "nativeRegister returned 0 — podcast FFI unavailable")
            }
        }
    }

    fun stop() = inner.stop()

    /**
     * Blocking (≤250 ms) drain of the kernel snapshot channel. Returns the
     * raw JSON envelope (or null on idle / shutdown). Decoded by
     * [com.podcast.app.android.PodcastKernelModel].
     */
    fun nextUpdate(): String? = inner.nextUpdate()

    /**
     * Unregister the podcast projection, then free the kernel session.
     * Call once on ViewModel.onCleared() — do not use the bridge after this.
     */
    fun free() {
        if (podcastHandle != 0L) {
            nativeUnregister(podcastHandle)
            podcastHandle = 0L
        }
        inner.free()
    }

    // -----------------------------------------------------------------------
    // Podcast-specific operations
    // -----------------------------------------------------------------------

    /**
     * Pull the current library as a JSON string (`{"podcasts":[…]}`).
     * Returns null if the podcast handle is not yet registered or the
     * serialization failed (D6).
     */
    fun snapshot(): String? {
        val h = podcastHandle
        return if (h != 0L) nativeSnapshot(h) else null
    }

    /**
     * Subscribe to a podcast feed. Returns `true` if a new podcast was added
     * (i.e. the library snapshot grew), `false` on dedup, invalid URL, or
     * any FFI error. The caller should re-fetch [snapshot] on `true`.
     *
     * @param feedUrl  RSS / Atom feed URL — must be a valid URL or the call
     *                 is a silent no-op (D6).
     * @param title    Optional display title; falls back to URL host if null.
     * @param author   Optional author string; falls back to empty if null.
     */
    fun subscribe(feedUrl: String, title: String? = null, author: String? = null): Boolean {
        val h = podcastHandle
        if (h == 0L) return false
        return nativeSubscribe(h, feedUrl, title ?: "", author ?: "")
    }

    /**
     * Remove a podcast by its ULID string. Idempotent — unknown IDs are a
     * silent no-op. The caller should re-fetch [snapshot] to refresh the UI.
     */
    fun unsubscribe(podcastId: String) {
        val h = podcastHandle
        if (h != 0L) nativeUnsubscribe(h, podcastId)
    }

    // -----------------------------------------------------------------------
    // JNI externals — podcast
    // -----------------------------------------------------------------------

    /** Register a PodcastHandle against the kernel session pointer. */
    private external fun nativeRegister(kernelHandle: Long): Long

    /** Drop the PodcastHandle. Must be called before nativeFree. */
    private external fun nativeUnregister(podcastHandle: Long)

    /** Serialize the current LibraryView to JSON. */
    private external fun nativeSnapshot(podcastHandle: Long): String?

    /**
     * Subscribe to a feed URL. Returns true if the library grew (new row),
     * false on dedup / failure.
     */
    private external fun nativeSubscribe(
        podcastHandle: Long,
        feedUrl: String,
        title: String,
        author: String,
    ): Boolean

    /** Unsubscribe a podcast by ULID string. Fire-and-forget. */
    private external fun nativeUnsubscribe(podcastHandle: Long, podcastId: String)

    // -----------------------------------------------------------------------
    // Feed ingest + episode list — T-podcast-android-3
    // -----------------------------------------------------------------------

    /**
     * Ingest raw RSS/Atom feed bytes for a subscribed podcast URL. The host
     * (Android) fetches the bytes via OkHttp (T-podcast-gap-3); this method
     * passes them to the Rust parser.
     *
     * Returns a JSON status string `{"ok":true,"episode_count":N}` or
     * `{"ok":false,"reason":"..."}`. Returns null on null handle.
     */
    fun ingestBytes(feedUrl: String, bytes: ByteArray): String? {
        val h = podcastHandle
        return if (h != 0L) nativeIngestBytes(h, feedUrl, bytes) else null
    }

    /**
     * Return the episode list for one podcast as a JSON string
     * (`{"episodes":[…]}`). Unknown ids return `{"episodes":[]}`.
     */
    fun episodes(podcastId: String): String? {
        val h = podcastHandle
        return if (h != 0L) nativeEpisodes(h, podcastId) else null
    }

    /** Ingest raw feed bytes. Returns JSON status or null on null handle. */
    private external fun nativeIngestBytes(
        podcastHandle: Long,
        feedUrl: String,
        bytes: ByteArray,
    ): String?

    /** Return episode list JSON for one podcast id. */
    private external fun nativeEpisodes(podcastHandle: Long, podcastId: String): String?
}
