package org.nmp.gallery.bridge

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import org.nmp.gallery.gallery.REGISTRY_SECTIONS
import org.nmp.gallery.gallery.RegistrySection
import org.nmp.gallery.gallery.parseRegistryJson
import org.nmp.gallery.registry.LoginBlockSignerState
import org.nmp.gallery.registry.NostrSignerInfo
import org.nmp.gallery.registry.ProfileWire

/**
 * Tiny ViewModel that owns the [KernelBridge] for the gallery's lifetime,
 * receives the kernel's FlatBuffers update frames via a JNI push callback
 * (issue #614 — D8 no-polling), and republishes the decoded profile slice as a
 * [StateFlow] for Compose.
 *
 * D5/D8: the kernel is the single source of truth. Profile data arrives via
 * the push callback only. Registry components claim pubkeys while visible and
 * resolved profile cards arrive pre-merged in `projections.resolved_profiles`
 * (the kernel performs the claimed/author/mention merge — this host does not).
 *
 * The registry section list is sourced once from `bridge.registryJson()` at
 * startup; [REGISTRY_SECTIONS] is used as a fallback if the JSON is absent or
 * unparseable.
 */
class GalleryModel : ViewModel() {

    private val bridge = KernelBridge()
    val showcase: GalleryShowcaseReferences =
        GalleryShowcaseReferences.decode(bridge.showcaseReferencesJson())

    private val _registrySections = MutableStateFlow<List<RegistrySection>>(
        parseRegistryJson(bridge.registryJson()) ?: REGISTRY_SECTIONS,
    )
    val registrySections: StateFlow<List<RegistrySection>> = _registrySections.asStateFlow()

    private val _profileMap = MutableStateFlow<Map<String, ProfileWire>>(emptyMap())
    val profileMap: StateFlow<Map<String, ProfileWire>> = _profileMap.asStateFlow()
    private val _claimedEvents = MutableStateFlow<Map<String, ClaimedEventWire>>(emptyMap())
    val claimedEvents: StateFlow<Map<String, ClaimedEventWire>> = _claimedEvents.asStateFlow()

    /**
     * ADR-0048 D6 — unified remote-signer health (`projections.signer_state`).
     * Null while no remote-signer session is active. Drives the login-block
     * showcase's inline status indicators.
     */
    private val _signerState = MutableStateFlow<LoginBlockSignerState?>(null)
    val signerState: StateFlow<LoginBlockSignerState?> = _signerState.asStateFlow()

    private val json: Json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    /**
     * ADR-0048 Stage 2 — the activity-owned NIP-55 host adapter
     * (`ExternalSignerCapabilityBridge.handleJson`). The push listener
     * hops each Rust-built request to Main (Intent launches require the
     * main thread). Null = no activity registered; the request is dropped
     * and degrades to a Rust-side timeout (D6).
     */
    @Volatile
    private var externalSignerHandler: ((requestJson: String) -> Unit)? = null

    init {
        bridge.galleryRegister()
        bridge.start(eventsPerSec = 0, visibleLimit = 80, emitHz = 4)
        // Issue #614 — push model (D8 no-polling): the kernel invokes this
        // callback on its update-listener thread for every snapshot frame.
        bridge.setUpdateListener { raw -> applyFrame(raw) }
        // Issue #1612 — push model (D8 no-polling): the kernel invokes this
        // callback on the capability-dispatch thread whenever a NIP-55 signer
        // request is dispatched, replacing the deleted blocking drain loop.
        bridge.setSignerRequestListener { requestJson ->
            dispatchSignerRequestToMain(requestJson)
        }
    }

    /** Register the activity-owned NIP-55 request handler. */
    fun registerExternalSignerHandler(handler: (requestJson: String) -> Unit) {
        externalSignerHandler = handler
    }

    /** Unregister on activity teardown (the launcher is being released). */
    fun unregisterExternalSignerHandler() {
        externalSignerHandler = null
    }

    /**
     * Begin a NIP-55 sign-in with the given detected signer. Kotlin reports
     * user intent only; Rust builds the `get_public_key` + permission-batch
     * request (D7) and routes it back through the registered bridge.
     */
    fun signInWithAmber(signer: NostrSignerInfo) {
        // Pass the explicit packageName (the APK identifier used for Intent
        // routing and the signer_package wire field). Falls back to
        // contentAuthority for signers where they coincide (e.g. Amber),
        // but the two fields are logically distinct — future signers
        // (e.g. Primal) have a packageName that differs from contentAuthority.
        bridge.signInNip55(signer.packageName ?: signer.contentAuthority)
    }

    /** Route a raw `ExternalSignerResponse` JSON back to the Rust driver. */
    fun deliverSignerResponse(responseJson: String) {
        bridge.deliverSignerResponse(responseJson)
    }

    /**
     * Make `pubkey` demand-driven on the kernel under a stable consumer id
     * so the kernel can reclaim slots when no view needs the profile.
     */
    fun claimProfile(pubkey: String, consumerId: String = CONSUMER_ID) {
        bridge.claimProfile(pubkey, consumerId)
    }

    fun releaseProfile(pubkey: String, consumerId: String = CONSUMER_ID) {
        bridge.releaseProfile(pubkey, consumerId)
    }

    fun claimEvent(uri: String, consumerId: String = CONSUMER_ID) {
        bridge.claimEvent(uri, consumerId)
    }

    fun releaseEvent(uri: String, consumerId: String = CONSUMER_ID) {
        bridge.releaseEvent(uri, consumerId)
    }

    fun dispatchAction(action: String, payload: String): String? =
        bridge.dispatchAction(action, payload)

    /**
     * Hop a pushed NIP-55 request (ADR-0048 Stage 2 / #1612) onto the main
     * thread before handing it to the activity-owned handler — the NIP-55
     * launch Intent requires the main thread. Rust invokes the listener on its
     * native capability-dispatch thread.
     */
    private fun dispatchSignerRequestToMain(requestJson: String) {
        externalSignerHandler?.let { handler ->
            viewModelScope.launch(Dispatchers.Main) { handler(requestJson) }
        } ?: android.util.Log.w("GalleryModel", "NIP-55 request dropped: no capability bridge registered")
    }

    /**
     * Decode one FlatBuffers snapshot frame. Profiles are read from
     * `projections.resolved_profiles` — ADR-0063 (#1671) now SOURCED from the
     * kernel's `refs.profile` row-delta projection (the resolve_ref output),
     * merged host-side across frames in the native session store. This host no
     * longer reimplements any precedence merge.
     */
    private fun applyFrame(raw: ByteArray) {
        val v = try {
            NmpUpdateFrameDecoder.decodeSnapshot(raw) { bridge.decodeSnapshotJson(it) }
        } catch (e: UpdateFrameDecodeException) {
            android.util.Log.w("GalleryModel", "drop frame: ${e.message}")
            return
        }
        val projections = (v["projections"] as? JsonObject) ?: return

        val assembled = mutableMapOf<String, ProfileWire>()

        // ADR-0063 (#1671): projections.resolved_profiles[pubkey] is a
        // ProfileWire-shaped entry materialised from the merged refs.profile
        // store. `npub_short` is derived from `npub` by the ProfileWire
        // constructor default when absent (same algorithm as before).
        (projections["resolved_profiles"] as? JsonObject)?.let { resolved ->
            for ((pubkey, el) in resolved) {
                val profile = runCatching {
                    json.decodeFromJsonElement<ProfileWire>(el)
                }.getOrNull() ?: continue
                assembled[pubkey] = profile
            }
        }

        if (assembled.isNotEmpty()) {
            _profileMap.value = _profileMap.value + assembled
        }

        val events = mutableMapOf<String, ClaimedEventWire>()
        (projections["claimed_events"] as? JsonObject)?.let { claimed ->
            for ((primaryId, el) in claimed) {
                val event = runCatching {
                    json.decodeFromJsonElement<ClaimedEventWire>(el)
                }.getOrNull() ?: continue
                events[primaryId] = event
            }
        }
        if (events.isNotEmpty()) {
            _claimedEvents.value = _claimedEvents.value + events
        }

        // ADR-0048 D6 — the unified remote-signer health slot. Absent =
        // no remote-signer session active (clears any prior state).
        _signerState.value = (projections["signer_state"] as? JsonObject)?.let { el ->
            runCatching { json.decodeFromJsonElement<LoginBlockSignerState>(el) }.getOrNull()
        }
    }

    override fun onCleared() {
        // Issue #614 — deregister the push listener before freeing so no frame
        // is delivered into a torn-down ViewModel. `free()` also quiesces and
        // clears the listener Rust-side; this is the explicit-ownership step.
        bridge.clearUpdateListener()
        // Issue #1612 — deregister the signer-request push listener on teardown.
        // No reader coroutine to join.
        bridge.clearSignerRequestListener()
        bridge.stop()
        bridge.free()
        super.onCleared()
    }

    companion object {
        const val CONSUMER_ID: String = "nmp-gallery"
    }
}

@Serializable
data class ClaimedEventWire(
    @SerialName("id") val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("kind") val kind: Long = 0L,
    @SerialName("created_at") val createdAt: Long = 0L,
    @SerialName("tags") val tags: List<List<String>> = emptyList(),
    @SerialName("content") val content: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
)
