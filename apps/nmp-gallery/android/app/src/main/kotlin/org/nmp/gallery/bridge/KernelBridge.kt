package org.nmp.gallery.bridge

import com.sun.jna.Pointer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import uniffi.nmp_uniffi.CapabilitySink
import uniffi.nmp_uniffi.NmpApp
import uniffi.nmp_uniffi.ProfileShape
import uniffi.nmp_uniffi.RefLiveness
import uniffi.nmp_uniffi.RefNamespace
import uniffi.nmp_uniffi.RefShape
import uniffi.nmp_uniffi.UpdateSink

/**
 * Push callback for kernel update frames (issue #614 - D8 no-polling).
 *
 * Rust invokes [onUpdate] from the kernel's update-listener thread (a native
 * background thread), NOT the Android main thread. [frame] is one FlatBuffers
 * snapshot frame.
 */
fun interface KernelUpdateListener {
    fun onUpdate(frame: ByteArray)
}

/**
 * Push callback for NIP-55 external-signer requests (issue #1612 - D8
 * no-polling).
 *
 * Rust invokes [onSignerRequest] from whichever thread dispatches the
 * `external_signer` capability, NOT the Android main thread. The NIP-55 launch
 * Intent must run on the main thread, so implementations marshal there
 * themselves. [requestJson] is one `ExternalSignerRequest` JSON for
 * `ExternalSignerCapabilityBridge.handleJson`.
 */
fun interface KernelSignerRequestListener {
    fun onSignerRequest(requestJson: String)
}

/**
 * Thin Android facade over the generated UniFFI `NmpApp` binding.
 *
 * Migrated framework lifecycle, update, capability, sign-in, and ref APIs use
 * UniFFI. Retained JNI calls are gallery-owned adapters only: composition
 * registration, static showcase/registry JSON, snapshot decoding, and
 * URI-to-ref adaptation.
 */
class KernelBridge {
    private var app: NmpApp? = null
    private var signerRequestListener: KernelSignerRequestListener? = null

    init {
        ensureLoaded()
        app = NmpApp()
    }

    /**
     * Boot the kernel + gallery projection.
     *
     * @param eventsPerSec Retained for source compatibility; UniFFI start owns
     * rendering limits only, so ingest throttling uses the runtime default.
     * @param visibleLimit Per-projection ring buffer size.
     * @param emitHz       Snapshot emission frequency (Hz).
     */
    @Suppress("UNUSED_PARAMETER")
    fun start(eventsPerSec: Int = 0, visibleLimit: Int = 80, emitHz: Int = 4) {
        val current = app ?: return
        current.start(visibleLimit.toUInt(), emitHz.toUInt())
    }

    fun stop() {
        app?.stop()
    }

    /** Register the gallery-specific projection on the kernel actor. */
    fun galleryRegister() {
        app?.withClonedPointer { nativeGalleryRegisterUniffi(it) }
    }

    fun showcaseReferencesJson(): String = nativeShowcaseReferencesJson()

    fun registryJson(): String = nativeRegistryJson()

    /**
     * ADR-0063 (#1671) - resolve a visible profile reference. Idempotent per
     * (pubkey, consumerId); matching [releaseProfileRef] required when the view
     * disappears.
     */
    fun resolveProfileRef(pubkey: String, consumerId: String) {
        app?.resolveProfileRef(pubkey, consumerId)
    }

    fun resolveProfileCard(pubkey: String, consumerId: String) {
        app?.resolveRef(
            RefNamespace.PROFILE,
            pubkey,
            consumerId,
            RefShape.Profile(ProfileShape.CARD),
            RefLiveness.CACHE_OK,
        )
    }

    fun releaseProfileRef(pubkey: String, consumerId: String) {
        app?.releaseProfileRef(pubkey, consumerId)
    }

    /** App-local URI adapter: JNI decodes [uri] then calls UniFFI app internals. */
    fun resolveEventRef(uri: String, consumerId: String) {
        app?.withClonedPointer { nativeResolveEventRef(it, uri, consumerId) }
    }

    /** App-local URI adapter: JNI decodes [uri] then releases the resolved key. */
    fun releaseEventRef(uri: String, consumerId: String) {
        app?.withClonedPointer { nativeReleaseEvent(it, uri, consumerId) }
    }

    /**
     * Register a push listener for kernel update frames (issue #614 - D8
     * no-polling).
     */
    fun setUpdateListener(listener: KernelUpdateListener) {
        app?.setUpdateSink(object : UpdateSink {
            override fun onUpdate(frame: ByteArray) {
                listener.onUpdate(frame)
            }
        })
    }

    /** Deregister the push listener. Safe when none is set. */
    fun clearUpdateListener() {
        app?.setUpdateSink(null)
    }

    /**
     * ADR-0048 Stage 2 - begin a NIP-55 sign-in routed to `signerPackage`
     * (null = let the OS resolver pick).
     */
    fun signInNip55(signerPackage: String?) {
        app?.signinNip55(signerPackage)
    }

    /**
     * ADR-0048 Stage 2 / issue #1612 - register a push listener for outbound
     * NIP-55 capability requests.
     */
    fun setSignerRequestListener(listener: KernelSignerRequestListener) {
        signerRequestListener = listener
        app?.setCapabilityCallback(object : CapabilitySink {
            override fun onCapabilityRequest(requestJson: String): String =
                handleCapabilityRequest(requestJson)
        })
    }

    /** Deregister the push listener set by [setSignerRequestListener]. */
    fun clearSignerRequestListener() {
        signerRequestListener = null
        app?.setCapabilityCallback(null)
    }

    /**
     * ADR-0048 Stage 2 - report a raw `ExternalSignerResponse` JSON back to
     * the Rust NIP-55 driver (D7: verbatim, Kotlin decides nothing).
     */
    fun deliverSignerResponse(responseJson: String) {
        app?.deliverExternalSignerResponse(responseJson)
    }

    fun free() {
        val current = app ?: return
        current.shutdown()
        current.close()
        app = null
        signerRequestListener = null
    }

    /**
     * ADR-0063 (#1671) - decode one FlatBuffers snapshot frame to the gallery
     * JSON shape. Returns null on decode failure (D6).
     */
    fun decodeSnapshotJson(frame: ByteArray): String? = nativeDecodeSnapshotJson(frame)

    private fun handleCapabilityRequest(requestJson: String): String {
        val parsed = runCatching { Json.parseToJsonElement(requestJson).jsonObject }.getOrNull()
        val namespace = parsed?.get("namespace")?.jsonPrimitive?.contentOrNull.orEmpty()
        val correlationId = parsed?.get("correlation_id")?.jsonPrimitive?.contentOrNull.orEmpty()
        if (namespace != "external_signer") {
            return capabilityErrorEnvelope(namespace, correlationId, "unsupported-on-android")
        }
        val payload = parsed?.get("payload_json")?.jsonPrimitive?.contentOrNull
            ?: return capabilityErrorEnvelope(namespace, correlationId, "missing-payload")
        val listener = signerRequestListener
            ?: return capabilityErrorEnvelope(namespace, correlationId, "session-closed")
        listener.onSignerRequest(payload)
        return capabilityEnvelope(namespace, correlationId, """{"status":"dispatched"}""")
    }

    private fun capabilityErrorEnvelope(
        namespace: String,
        correlationId: String,
        reason: String,
    ): String = capabilityEnvelope(
        namespace,
        correlationId,
        """{"status":"error","os_status":-50,"reason":"$reason"}""",
    )

    private fun capabilityEnvelope(namespace: String, correlationId: String, resultJson: String): String =
        buildJsonObject {
            put("namespace", namespace)
            put("correlation_id", correlationId)
            put("result_json", resultJson)
        }.toString()

    private fun NmpApp.withClonedPointer(block: (Long) -> Unit) {
        val pointer = uniffiClonePointer()
        block(Pointer.nativeValue(pointer))
    }

    private external fun nativeGalleryRegisterUniffi(arcPtr: Long)
    private external fun nativeShowcaseReferencesJson(): String
    private external fun nativeRegistryJson(): String
    private external fun nativeResolveEventRef(arcPtr: Long, uri: String, consumerId: String)
    private external fun nativeReleaseEvent(arcPtr: Long, uri: String, consumerId: String)

    companion object {
        @JvmStatic
        private external fun nativeDecodeSnapshotJson(frame: ByteArray): String?

        private val loaded: Boolean = run {
            System.loadLibrary("nmp_app_gallery")
            System.setProperty("uniffi.component.nmp_uniffi.libraryOverride", "nmp_app_gallery")
            true
        }

        private fun ensureLoaded() {
            loaded
        }
    }
}
