package org.nmp.gallery.bridge

/**
 * Push callback for kernel update frames (issue #614 — D8 no-polling).
 *
 * Rust invokes [onUpdate] from the kernel's update-listener thread (a native
 * background thread), NOT the Android main thread. [frame] is one FlatBuffers
 * snapshot frame. Mirrors the Chirp `KernelUpdateListener`.
 */
fun interface KernelUpdateListener {
    fun onUpdate(frame: ByteArray)
}

/**
 * Push callback for NIP-55 external-signer requests (issue #1612 — D8
 * no-polling; replaces the deleted `nativeNextSignerRequest` blocking drain).
 *
 * Rust invokes [onSignerRequest] from whichever thread dispatches the
 * `external_signer` capability (a native background thread), NOT the Android
 * main thread. The NIP-55 launch Intent must run on the main thread, so
 * implementations marshal there themselves. [requestJson] is one
 * `ExternalSignerRequest` JSON for `ExternalSignerCapabilityBridge.handleJson`.
 * Mirrors the Chirp `KernelSignerRequestListener`.
 */
fun interface KernelSignerRequestListener {
    fun onSignerRequest(requestJson: String)
}

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
 * UI thread and receives pushed snapshots via [setUpdateListener].
 */
class KernelBridge {
    private var handle: Long = 0

    init {
        ensureLoaded()
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
     * ADR-0063 (#1671) — resolve a visible profile reference (supersedes the
     * deleted `nativeClaimProfile`). Origin-blind: the gallery resolves every
     * visible author at `profile.ref` / `CacheOk` (inline avatars/names only).
     * Idempotent per (pubkey, consumerId); matching [releaseProfile] required
     * when the view disappears. The resolved kind:0 flows back through the
     * `refs.profile` row-delta projection.
     */
    fun claimProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) {
            nativeResolveRef(
                handle,
                REF_NS_PROFILE,
                pubkey,
                consumerId,
                REF_SHAPE_PROFILE_REF,
                REF_LIVENESS_CACHE_OK,
            )
        }
    }

    fun releaseProfile(pubkey: String, consumerId: String) {
        if (handle != 0L) nativeReleaseRef(handle, REF_NS_PROFILE, pubkey, consumerId)
    }

    fun claimEvent(uri: String, consumerId: String) {
        if (handle != 0L) nativeClaimEvent(handle, uri, consumerId)
    }

    fun releaseEvent(uri: String, consumerId: String) {
        if (handle != 0L) nativeReleaseEvent(handle, uri, consumerId)
    }

    /**
     * Register a push listener for kernel update frames (issue #614 — D8
     * no-polling; replaces the former blocking `nextUpdate` drain).
     *
     * [listener] receives each FlatBuffers snapshot frame on the kernel's
     * update-listener thread (a native background thread). Pass a new listener
     * to swap; call [clearUpdateListener] on teardown before [free]. D6: a
     * null/dead handle is a no-op.
     */
    fun setUpdateListener(listener: KernelUpdateListener) {
        if (handle != 0L) nativeSetUpdateListener(handle, listener)
    }

    /** Deregister the push listener. Safe when none is set; D6 no-op on dead handle. */
    fun clearUpdateListener() {
        if (handle != 0L) nativeClearUpdateListener(handle)
    }

    /**
     * Dispatch a typed action through the kernel's action seam. Payload is
     * an action-specific JSON object; return value is the JSON envelope the
     * action handler produced (or null on transport failure).
     */
    fun dispatchAction(action: String, payload: String): String? =
        if (handle != 0L) nativeDispatchAction(handle, action, payload) else null

    /**
     * ADR-0048 Stage 2 — begin a NIP-55 sign-in routed to `signerPackage`
     * (null = let the OS resolver pick). Rust builds the `get_public_key` +
     * permission-batch request and dispatches it through the capability socket;
     * the request is pushed to the registered [KernelSignerRequestListener]
     * (see [setSignerRequestListener]).
     */
    fun signInNip55(signerPackage: String?) {
        if (handle != 0L) nativeSignInNip55(handle, signerPackage)
    }

    /**
     * ADR-0048 Stage 2 / issue #1612 — register a push listener for outbound
     * NIP-55 capability requests (D8 — no polling; replaces the deleted
     * `nativeNextSignerRequest` blocking drain).
     *
     * [listener] receives each `ExternalSignerRequest` JSON on the Rust
     * capability-dispatch thread — a native background thread, NOT the main
     * thread. The NIP-55 launch Intent requires the main thread, so the
     * implementation must marshal there itself. Pass a new listener to swap;
     * call [clearSignerRequestListener] on teardown before [free]. D6: a
     * null/dead handle is a no-op.
     */
    fun setSignerRequestListener(listener: KernelSignerRequestListener) {
        if (handle != 0L) nativeSetSignerRequestListener(handle, listener)
    }

    /**
     * Deregister the push listener set by [setSignerRequestListener]. Safe to
     * call when none is registered. D6: a null/dead handle is a no-op.
     */
    fun clearSignerRequestListener() {
        if (handle != 0L) nativeClearSignerRequestListener(handle)
    }

    /**
     * ADR-0048 Stage 2 — report a raw `ExternalSignerResponse` JSON back to
     * the Rust NIP-55 driver (D7: verbatim, Kotlin decides nothing).
     */
    fun deliverSignerResponse(responseJson: String) {
        if (handle != 0L) nativeDeliverSignerResponse(handle, responseJson)
    }

    fun free() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0
        }
    }

    /**
     * ADR-0063 (#1671) — decode one FlatBuffers snapshot frame to the gallery
     * JSON shape, merging the frame's `refs.profile` row-delta batch into this
     * session's persistent store first. Instance-scoped (the store lives in the
     * native session keyed by [handle]). Returns null on a dead handle / decode
     * failure (D6).
     */
    fun decodeSnapshotJson(frame: ByteArray): String? =
        if (handle != 0L) Companion.nativeDecodeSnapshotJson(handle, frame) else null

    private external fun nativeNew(): Long
    private external fun nativeFree(handle: Long)
    private external fun nativeGalleryRegister(handle: Long)
    private external fun nativeShowcaseReferencesJson(): String
    private external fun nativeRegistryJson(): String
    private external fun nativeStart(handle: Long, eventsPerSec: Int, visibleLimit: Int, emitHz: Int)
    private external fun nativeStop(handle: Long)
    private external fun nativeResolveRef(
        handle: Long,
        namespace: Int,
        key: String,
        consumerId: String,
        shape: Int,
        liveness: Int,
    )
    private external fun nativeReleaseRef(handle: Long, namespace: Int, key: String, consumerId: String)
    private external fun nativeClaimEvent(handle: Long, uri: String, consumerId: String)
    // nativeDecodeSnapshotJson lives in the companion (static JNI) so it can be
    // reused without an instance; the [handle] selects the session's store.
    private external fun nativeReleaseEvent(handle: Long, uri: String, consumerId: String)
    private external fun nativeSetUpdateListener(handle: Long, listener: KernelUpdateListener)
    private external fun nativeClearUpdateListener(handle: Long)
    private external fun nativeDispatchAction(handle: Long, action: String, payload: String): String?
    private external fun nativeSignInNip55(handle: Long, signerPackage: String?)
    private external fun nativeSetSignerRequestListener(handle: Long, listener: KernelSignerRequestListener)
    private external fun nativeClearSignerRequestListener(handle: Long)
    private external fun nativeDeliverSignerResponse(handle: Long, responseJson: String)

    companion object {
        @JvmStatic
        private external fun nativeDecodeSnapshotJson(handle: Long, frame: ByteArray): String?

        // ADR-0063 (#1671) FFI integer codes for resolve_ref / release_ref.
        /** `namespace` — the profile resolver. */
        private const val REF_NS_PROFILE: Int = 0
        /** `shape` — `profile.ref` (`{pubkey, display_name, picture_url}`; avatar/name). */
        private const val REF_SHAPE_PROFILE_REF: Int = 0
        /** `liveness` — `CacheOk` (background; no per-row tailing sub). */
        private const val REF_LIVENESS_CACHE_OK: Int = 0

        private val loaded: Boolean = run {
            System.loadLibrary("nmp_app_gallery")
            true
        }

        private fun ensureLoaded() {
            loaded
        }
    }
}
