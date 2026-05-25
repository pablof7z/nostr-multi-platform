package org.nmp.android

/**
 * Thin JNI wrapper around `libnmp_android_ffi.so`, which links the SAME
 * `nmp_app_*` Rust kernel the iOS app consumes. Direct mirror of
 * `ios/Chirp/.../KernelBridge.swift`'s `KernelHandle`.
 *
 * Doctrine: no business logic or cached state (D5/D8). Errors never cross FFI
 * (D6) — natives return only a handle / string / void; outcomes arrive in the
 * next JSON snapshot. The Rust side lives in `crates/nmp-android-ffi` and calls
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

    /** Blocking (≤250 ms) drain of the kernel snapshot channel; null on idle. */
    fun nextUpdate(): String? = if (handle != 0L) nativeNextUpdate(handle) else null

    /** Full Chirp modular timeline projection produced by `nmp-app-chirp`. */
    fun chirpSnapshot(): String? = if (handle != 0L) nativeChirpSnapshot(handle) else null

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
    private external fun nativeNextUpdate(handle: Long): String?
    private external fun nativeChirpSnapshot(handle: Long): String?
    private external fun nativeFree(handle: Long)
}
