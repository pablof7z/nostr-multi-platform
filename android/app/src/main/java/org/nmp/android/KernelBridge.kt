package org.nmp.android

/**
 * Thin JNI wrapper around `libnmp_android_ffi.so`, which links the SAME
 * `nmp_app_*` Rust kernel the iOS app consumes. Direct mirror of
 * `ios/NmpPulse/.../KernelBridge.swift`'s `KernelHandle`.
 *
 * Doctrine: no business logic or cached state (D5/D8). Errors never cross FFI
 * (D6) — natives return only a handle / string / void; outcomes arrive in the
 * next JSON snapshot. See `android/JNI-CONTRACT.md`.
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

    /** Blocking (≤250 ms) drain of the kernel snapshot channel; null on idle. */
    fun nextUpdate(): String? = if (handle != 0L) nativeNextUpdate(handle) else null

    fun free() {
        if (handle != 0L) {
            nativeFree(handle)
            handle = 0
        }
    }

    private external fun nativeNew(): Long
    private external fun nativeStart(handle: Long, visibleLimit: Int, emitHz: Int)
    private external fun nativeStop(handle: Long)
    private external fun nativeNextUpdate(handle: Long): String?
    private external fun nativeFree(handle: Long)
}
