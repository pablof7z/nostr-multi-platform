package org.nmp.gallery.bridge

/**
 * Thin JNI wrapper around `libnmp_app_gallery.so` — the gallery-specific
 * Rust shim that links the SAME `nmp-core` kernel that Chirp / iOS consume.
 *
 * Doctrine: no business logic or cached state (D5/D8). Errors never cross
 * FFI (D6) — natives return only a handle / bytes / void; outcomes arrive
 * in the next FlatBuffers update frame. The Rust side is in
 * `apps/nmp-gallery/nmp-app-gallery`; it MUST export
 * JNI symbols named `Java_org_nmp_gallery_bridge_KernelBridge_<methodName>`
 * to match this Kotlin class.
 *
 * This bridge intentionally has NO OkHttp / Ktor / WebSocket code. Every
 * relay connection lives inside the Rust kernel; Kotlin only owns the
 * UI thread and drains the snapshot channel.
 */
class KernelBridge {
    private var handle: Long = 0

    init {
        System.loadLibrary("nmp_app_gallery")
        handle = nativeNew()
    }

    /**
     * Boot the kernel + gallery projection.
     *
     * @param eventsPerSec Optional Rust ingest cap (0 disables).
     * @param visibleLimit Per-projection ring buffer size.
     * @param emitHz       Snapshot emission frequency (Hz). Chirp uses 4 Hz.
     */
    fun start(eventsPerSec: Int = 0, visibleLimit: Int = 80, emitHz: Int = 4) {
        if (handle != 0L) nativeStart(handle, eventsPerSec, visibleLimit, emitHz)
    }

    fun stop() {
        if (handle != 0L) nativeStop(handle)
    }

    /** Register the gallery-specific projection on the kernel actor. */
    fun galleryRegister() {
        if (handle != 0L) nativeGalleryRegister(handle)
    }

    fun showcaseReferencesJson(): String = nativeShowcaseReferencesJson()

    fun registryJson(): String = nativeRegistryJson()

    /**
     * Open the author view for [pubkey]. Triggers kind:0 + kind:10002 fetch
     * and populates `projections.author_view` on every snapshot tick.
     * Mirrors `nmp_app_open_author` from the iOS shell.
     */
    fun openAuthor(pubkey: String) {
        if (handle != 0L) nativeOpenAuthor(handle, pubkey)
    }

    /**
     * Demand-driven kind:0 fetch claim — see KernelBridge.swift /
     * `nmp_app_claim_profile`. Idempotent per (pubkey, consumerId);
     * matching [releaseProfile] required when the view disappears.
     */
    fun claimProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) nativeClaimProfile(handle, pubkey, consumerId)
    }

    fun releaseProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) nativeReleaseProfile(handle, pubkey, consumerId)
    }

    fun claimEvent(uri: String, consumerId: String) {
        if (handle != 0L) nativeClaimEvent(handle, uri, consumerId)
    }

    fun releaseEvent(uri: String, consumerId: String) {
        if (handle != 0L) nativeReleaseEvent(handle, uri, consumerId)
    }

    /**
     * Blocking drain of the update-frame channel. `timeoutMs` caps the wait
     * so the Kotlin reader coroutine can react to cancellation.
     *
     * Return contract (V-57 P5):
     * * `null` — idle tick (`RecvTimeoutError::Timeout` on the Rust side).
     *   The caller should loop back into `nextUpdate` immediately.
     * * `ByteArray` (non-empty) — one FlatBuffers snapshot frame.
     * * Throws [IllegalStateException] — the snapshot channel has been
     *   closed (`RecvTimeoutError::Disconnected`; the boxed `Sender` in the
     *   Rust `GallerySession` was dropped, typically as part of `free()`).
     *   The caller MUST stop polling — looping after a disconnect spins
     *   the CPU on a dead channel.
     */
    fun nextUpdate(timeoutMs: Long = 250L): ByteArray? =
        if (handle != 0L) nativeNextUpdate(handle, timeoutMs) else null

    /**
     * Minimal status envelope from the gallery projection
     * (`{schema, alive, projections:{}}`). Live profile data is NOT in this
     * envelope — it arrives via [nextUpdate] from the push callback. Used
     * only for diagnostic / readiness checks.
     */
    fun gallerySnapshot(): String? =
        if (handle != 0L) nativeGallerySnapshot(handle) else null

    /**
     * Dispatch a typed action through the kernel's action seam. Payload is
     * an action-specific JSON object; return value is the JSON envelope the
     * action handler produced (or null on transport failure).
     */
    fun dispatchAction(action: String, payload: String): String? =
        if (handle != 0L) nativeDispatchAction(handle, action, payload) else null

    fun free() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0
        }
    }

    private external fun nativeNew(): Long
    private external fun nativeFree(handle: Long)
    private external fun nativeGalleryRegister(handle: Long)
    private external fun nativeShowcaseReferencesJson(): String
    private external fun nativeRegistryJson(): String
    private external fun nativeOpenAuthor(handle: Long, pubkey: String)
    private external fun nativeStart(handle: Long, eventsPerSec: Int, visibleLimit: Int, emitHz: Int)
    private external fun nativeStop(handle: Long)
    private external fun nativeClaimProfile(handle: Long, pubkey: String, consumerId: String)
    private external fun nativeReleaseProfile(handle: Long, pubkey: String, consumerId: String)
    private external fun nativeClaimEvent(handle: Long, uri: String, consumerId: String)
    private external fun nativeReleaseEvent(handle: Long, uri: String, consumerId: String)
    private external fun nativeNextUpdate(handle: Long, timeoutMs: Long): ByteArray?
    private external fun nativeGallerySnapshot(handle: Long): String?
    private external fun nativeDispatchAction(handle: Long, action: String, payload: String): String?
}
