package org.nmp.gallery.bridge

import com.sun.jna.Pointer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import uniffi.nmp_uniffi.CapabilitySink
import uniffi.nmp_uniffi.NmpApp
import uniffi.nmp_uniffi.UpdateSink

/**
 * Push callback for kernel update frames (issue #614 — D8 no-polling).
 *
 * Rust invokes [onUpdate] from the kernel's update-listener thread (a native
 * background thread), NOT the Android main thread. [frame] is one FlatBuffers
 * snapshot frame.
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
 * Mirrors the update-listener push contract.
 */
fun interface KernelSignerRequestListener {
    fun onSignerRequest(requestJson: String)
}

/**
 * M14 shell-2 — UniFFI NmpApp wrapper for the NmpGallery Android shell.
 *
 * Replaces the raw JNI Long-handle session (M14 shell-2 migration). The
 * [NmpApp] UniFFI class owns the kernel Arc; JNA resolves all `uniffi_nmp_uniffi_*`
 * symbols against `libnmp_app_gallery.so` via the library-override property set
 * in the companion [init] block.
 *
 * Doctrine: no business logic or cached state (D5/D8). Errors never cross FFI
 * (D6) — outcomes arrive in the next FlatBuffers update frame. Gallery-owned
 * C-ABI symbols (`nativeShowcaseReferencesJson`, `nativeRegistryJson`,
 * `nativeDecodeSnapshotJson`, `nativeResolveEventRef`, `nativeReleaseEvent`)
 * remain as JNI — they have no UniFFI counterpart on the NmpApp interface.
 *
 * There is NO OkHttp / Ktor / WebSocket code in this bridge. Every relay
 * connection lives inside the Rust kernel; Kotlin only owns the UI thread and
 * receives pushed snapshots via [setUpdateListener].
 */
class KernelBridge {
    private val app: NmpApp

    init {
        ensureLoaded()
        app = NmpApp()
        // Register the gallery-specific composition (installs substrate, protocol,
        // and projection defaults) via the UniFFI Arc pointer. Must happen before
        // start() to wire gallery projections before the actor loop begins.
        nativeGalleryRegisterUniffi(Pointer.nativeValue(app.uniffiClonePointer()))
        // Wire the NIP-55 external-signer capability transport. Idempotent.
        app.initExternalSigner()
    }

    /**
     * Start the kernel + gallery projection.
     *
     * Adds the embedded showcase relays (same set as the old `nativeStart`
     * C-ABI path) then spawns the actor loop.
     *
     * @param eventsPerSec Ignored (retained for API compat with old C-ABI callers).
     * @param visibleLimit Per-projection ring buffer size.
     * @param emitHz       Snapshot emission frequency (Hz).
     */
    @Suppress("UNUSED_PARAMETER")
    fun start(eventsPerSec: Int = 0, visibleLimit: Int = 80, emitHz: Int = 4) {
        // Add showcase relays — previously done inside the C-ABI nativeStart.
        val refs = GalleryShowcaseReferences.decode(showcaseReferencesJson())
        refs.relays.forEach { relay -> app.addRelay(relay.url, relay.role) }
        app.start(visibleLimit.toUInt(), emitHz.toUInt())
    }

    fun stop() {
        app.stop()
    }

    /**
     * No-op: gallery composition is registered in [init] via
     * [nativeGalleryRegisterUniffi]. Kept for API compatibility with callers
     * that call `bridge.galleryRegister()` before `bridge.start()`.
     */
    fun galleryRegister() {
        // Gallery composition registered in init — nothing to do here.
    }

    fun showcaseReferencesJson(): String = nativeShowcaseReferencesJson()

    fun registryJson(): String = nativeRegistryJson()

    /**
     * ADR-0063 (#1671) — resolve a visible profile reference. Origin-blind:
     * the gallery resolves every visible author at `profile.ref` / `CacheOk`.
     * Idempotent per (pubkey, consumerId); matching [releaseProfileRef] required
     * when the view disappears.
     */
    fun resolveProfileRef(pubkey: String, consumerId: String) {
        app.resolveProfileRef(pubkey, consumerId)
    }

    fun releaseProfileRef(pubkey: String, consumerId: String) {
        app.releaseProfileRef(pubkey, consumerId)
    }

    /** URI adapter: decodes [uri] via JNI and calls resolve_event_embed_with_metadata. */
    fun resolveEventRef(uri: String, consumerId: String) {
        nativeResolveEventRef(Pointer.nativeValue(app.uniffiClonePointer()), uri, consumerId)
    }

    /** URI adapter: decodes [uri] via JNI and calls release_event_ref. */
    fun releaseEventRef(uri: String, consumerId: String) {
        nativeReleaseEvent(Pointer.nativeValue(app.uniffiClonePointer()), uri, consumerId)
    }

    /**
     * Register a push listener for kernel update frames (issue #614 — D8
     * no-polling; replaces the former blocking `nextUpdate` drain).
     *
     * [listener] receives each FlatBuffers snapshot frame on the kernel's
     * update-listener thread (a native background thread). Pass a new listener
     * to swap; call [clearUpdateListener] on teardown before [free]. D6: a
     * dead app is a no-op.
     */
    fun setUpdateListener(listener: KernelUpdateListener) {
        app.setUpdateSink(object : UpdateSink {
            override fun onUpdate(frame: ByteArray) {
                listener.onUpdate(frame)
            }
        })
    }

    /** Deregister the push listener. Safe when none is set. */
    fun clearUpdateListener() {
        app.setUpdateSink(null)
    }

    /**
     * ADR-0048 Stage 2 — begin a NIP-55 sign-in routed to [signerPackage]
     * (null = let the OS resolver pick). Rust builds the `get_public_key` +
     * permission-batch request and dispatches it through the capability socket;
     * the request is pushed to the registered [KernelSignerRequestListener].
     */
    fun signInNip55(signerPackage: String?) {
        app.signinNip55(signerPackage)
    }

    /**
     * ADR-0048 Stage 2 / issue #1612 — register a push listener for outbound
     * NIP-55 capability requests (D8 no-polling).
     *
     * The UniFFI [CapabilitySink.onCapabilityRequest] receives a full capability
     * envelope JSON. This implementation extracts the `payload_json` field (the
     * `ExternalSignerRequest` body) and forwards it to [listener.onSignerRequest]
     * — preserving the pre-M14 contract so callers need no changes.
     */
    fun setSignerRequestListener(listener: KernelSignerRequestListener) {
        app.setCapabilityCallback(object : CapabilitySink {
            private val json = Json { ignoreUnknownKeys = true }
            override fun onCapabilityRequest(requestJson: String): String {
                return try {
                    val root = json.parseToJsonElement(requestJson).jsonObject
                    val namespace = root["namespace"]?.jsonPrimitive?.content ?: ""
                    val correlationId = root["correlation_id"]?.jsonPrimitive?.content ?: ""
                    if (namespace != "external_signer") {
                        return """{"namespace":"$namespace","correlation_id":"$correlationId","result_json":null,"error":"unsupported-on-android"}"""
                    }
                    val payload = root["payload_json"]?.jsonPrimitive?.content ?: ""
                    listener.onSignerRequest(payload)
                    """{"namespace":"external_signer","correlation_id":"$correlationId","result_json":"{\"status\":\"dispatched\"}"}"""
                } catch (_: Exception) {
                    """{"error":"capability-parse-error"}"""
                }
            }
        })
    }

    /**
     * Deregister the push listener set by [setSignerRequestListener]. Safe to
     * call when none is registered.
     */
    fun clearSignerRequestListener() {
        app.setCapabilityCallback(null)
    }

    /**
     * ADR-0048 Stage 2 — report a raw `ExternalSignerResponse` JSON back to
     * the Rust NIP-55 driver (D7: verbatim, Kotlin decides nothing).
     */
    fun deliverSignerResponse(responseJson: String) {
        app.deliverExternalSignerResponse(responseJson)
    }

    fun free() {
        app.close()
    }

    /**
     * ADR-0063 (#1671) — decode one FlatBuffers snapshot frame to the gallery
     * JSON shape, merging the frame's `refs.profile` / `refs.event` row-delta
     * batch into the process-global store first. Returns null on a decode
     * failure (D6).
     */
    fun decodeSnapshotJson(frame: ByteArray): String? = nativeDecodeSnapshotJson(frame)

    // ── Gallery-owned JNI (out of M14 UniFFI scope) ──────────────────────

    /** M14 shell-2 — register gallery composition via UniFFI Arc pointer. */
    private external fun nativeGalleryRegisterUniffi(arcPtr: Long)

    /** URI adapter: decodes nostr: URI and resolves event embed with metadata. */
    private external fun nativeResolveEventRef(arcPtr: Long, uri: String, consumerId: String)

    /** URI adapter: decodes nostr: URI key and releases event ref. */
    private external fun nativeReleaseEvent(arcPtr: Long, uri: String, consumerId: String)

    /** Static gallery reference JSON embedded by Rust. */
    private external fun nativeShowcaseReferencesJson(): String

    /** Static registry JSON embedded by Rust. */
    private external fun nativeRegistryJson(): String

    /**
     * Decode one FlatBuffers frame → gallery snapshot JSON. Uses a
     * process-global GalleryRefStores (no session handle needed post-M14).
     */
    private external fun nativeDecodeSnapshotJson(frame: ByteArray): String?

    companion object {
        private val loaded: Boolean = run {
            // Tell JNA (UniFFI runtime) to load the uniffi_nmp_uniffi_* symbols
            // from libnmp_app_gallery.so — the single .so that bundles both the
            // gallery C-ABI and the UniFFI surface.
            System.setProperty(
                "uniffi.component.nmp_uniffi.libraryOverride",
                "nmp_app_gallery",
            )
            System.loadLibrary("nmp_app_gallery")
            true
        }

        private fun ensureLoaded() {
            loaded
        }
    }
}
