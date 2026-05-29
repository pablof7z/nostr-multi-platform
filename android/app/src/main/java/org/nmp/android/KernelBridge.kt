package org.nmp.android

/**
 * Thin JNI wrapper around `libnmp_android_ffi.so`, which links the SAME
 * `nmp_app_*` Rust kernel the iOS app consumes. Direct mirror of
 * `ios/Chirp/.../KernelBridge.swift`'s `KernelHandle`.
 *
 * Doctrine: no business logic or cached state (D5/D8). Errors never cross FFI
 * (D6) — natives return only a handle / bytes / void; outcomes arrive in the
 * next update frame. The Rust side lives in `crates/nmp-android-ffi` and calls
 * into `nmp-ffi`/`nmp-app-chirp` through Rust paths.
 */
class KernelBridge {
    private var handle: Long = 0

    init {
        System.loadLibrary("nmp_android_ffi")
        handle = nativeNew()
    }

    fun start(visibleLimit: Int = 80, emitHz: Int = 4) {
        if (handle != 0L) nativeStart(handle, visibleLimit, emitHz)
    }

    fun stop() {
        if (handle != 0L) nativeStop(handle)
    }

    fun openTimeline() {
        if (handle != 0L) nativeOpenTimeline(handle)
    }

    fun createLocalAccount(displayName: String = "Android User") {
        if (handle != 0L) nativeCreateLocalAccount(handle, displayName)
    }

    /**
     * Blocking (≤250 ms) drain of the kernel update channel.
     *
     * Return contract (mirrors PR #644 / V-57 P5 for nmp-gallery):
     * * `null` — idle tick (`RecvTimeoutError::Timeout` on the Rust side).
     *   The caller should loop back into `nextUpdate` immediately.
     * * Non-null [ByteArray] — one FlatBuffers `UpdateFrame` (file_identifier "NMPU").
     *   Decode with [KernelUpdateFrameDecoder].
     * * Throws [IllegalStateException] — the update channel has been closed
     *   (`RecvTimeoutError::Disconnected`; the boxed `Sender` in the Rust
     *   `Session` was dropped, typically as part of `free()`). The caller MUST
     *   stop polling — looping after a disconnect spins the CPU on a dead channel.
     */
    fun nextUpdate(): ByteArray? = if (handle != 0L) nativeNextUpdate(handle) else null

    /**
     * Demand-driven profile fetch claim: the UI is rendering [pubkey] under
     * [consumerId]; the kernel batches a kind:0 REQ against the indexer lane
     * (or the author's NIP-65 write set once known). Direct mirror of iOS
     * `KernelHandle.claimProfile(pubkey:consumerId:)`.
     *
     * Idempotent — duplicate calls with the same [consumerId] are no-ops. The
     * matching [releaseProfile] must be called when the view disappears so
     * the kernel can reclaim the claim slot.
     */
    fun claimProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) nativeClaimProfile(handle, pubkey, consumerId)
    }

    /**
     * Demand-driven profile fetch release: the UI no longer needs [pubkey]
     * under [consumerId]. When the last consumer releases the kernel
     * reclaims the profile-claim entry; subsequent kind:0 fetches are
     * gated by a fresh [claimProfile].
     */
    fun releaseProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) nativeReleaseProfile(handle, pubkey, consumerId)
    }

    /**
     * Dispatch a named action through the action registry.
     *
     * Returns a JSON response:
     * * `{"correlation_id":"<32-hex>"}` — the action was accepted and assigned
     *   a correlation id.
     * * `{"error":"<message>"}` — the action was rejected (invalid arguments,
     *   unknown namespace, malformed JSON).
     * * `"{}"` — null handle or internal error.
     */
    fun dispatchAction(namespace: String, actionJson: String): String =
        if (handle != 0L) nativeDispatchAction(handle, namespace, actionJson) else "{}"

    /**
     * Open a thread by note ID. The kernel batches a corresponding
     * kind:1 REQ and opens the thread timeline for rendering.
     *
     * D6: null handle or invalid note_id is a silent no-op.
     */
    fun openThread(noteId: String) {
        if (handle != 0L) nativeOpenThread(handle, noteId)
    }

    /**
     * Open an author profile by pubkey. The kernel batches a corresponding
     * kind:0 REQ and opens the author timeline for rendering.
     *
     * D6: null handle or invalid pubkey is a silent no-op.
     */
    fun openAuthor(pubkey: String) {
        if (handle != 0L) nativeOpenAuthor(handle, pubkey)
    }

    /**
     * Expose the raw Android JNI Session pointer (`jlong`) to same-process
     * Android bridge extensions. Returns 0 if the bridge was freed. Callers
     * must not store this value beyond the lifetime of this bridge.
     */
    fun rawHandle(): Long = handle

    fun free() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0
        }
    }

    private external fun nativeNew(): Long
    private external fun nativeStart(handle: Long, visibleLimit: Int, emitHz: Int)
    private external fun nativeOpenTimeline(handle: Long)
    private external fun nativeCreateLocalAccount(handle: Long, displayName: String)
    private external fun nativeStop(handle: Long)
    private external fun nativeNextUpdate(handle: Long): ByteArray?
    private external fun nativeClaimProfile(handle: Long, pubkey: String, consumerId: String)
    private external fun nativeReleaseProfile(handle: Long, pubkey: String, consumerId: String)
    private external fun nativeDispatchAction(handle: Long, namespace: String, actionJson: String): String
    private external fun nativeOpenThread(handle: Long, noteId: String)
    private external fun nativeOpenAuthor(handle: Long, pubkey: String)
    private external fun nativeFree(handle: Long)
}
