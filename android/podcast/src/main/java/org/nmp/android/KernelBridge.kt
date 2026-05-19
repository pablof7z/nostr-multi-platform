package org.nmp.android

/**
 * Thin JNI wrapper around `libnmp_android_ffi.so`, which links the SAME
 * `nmp_app_*` Rust kernel symbol set as the iOS app consumes.
 *
 * **Why this lives in `org.nmp.android` inside the podcast module (T157):**
 * the JNI entrypoint names in `crates/nmp-android-ffi/src/lib.rs` are
 * `Java_org_nmp_android_KernelBridge_nativeXxx`. JNI symbol resolution
 * matches the fully-qualified class name (package + class), not the Gradle
 * module the class is compiled in. Putting this file in the podcast APK
 * under the same package lets both apps reuse the SAME .so without adding
 * parallel `com_podcast_app_android_*` exports to the Rust shim. The
 * podcast app's [com.podcast.app.android.bridge.PodcastKernelBridge]
 * delegates here.
 *
 * This is a verbatim re-paste of `android/app/src/main/java/.../KernelBridge.kt`.
 * Both copies must stay in lockstep — a future refactor can extract them into
 * a shared library module if a third app appears. For two apps the duplication
 * is the simpler choice (no extra Gradle module, no cross-cutting refactor).
 *
 * Doctrine: pure transport (D5/D6). No business logic, no cached state,
 * errors never cross FFI.
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

    /**
     * Expose the raw kernel Session pointer (jlong) so per-app bridges
     * (e.g. [com.podcast.app.android.bridge.PodcastKernelBridge]) can call
     * `nmp_app_podcast_register(app)` by passing the session handle into Rust,
     * which extracts `session.app`. Returns 0 if the bridge was freed.
     * Callers must not store this value beyond the lifetime of this bridge.
     *
     * Lockstep: `android/app/src/main/java/org/nmp/android/KernelBridge.kt`
     * must carry the same method (T-podcast-android-2).
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
    private external fun nativeStop(handle: Long)
    private external fun nativeNextUpdate(handle: Long): String?
    private external fun nativeFree(handle: Long)
}
