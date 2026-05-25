package org.nmp.gallery.bridge

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonPrimitive
import org.nmp.gallery.registry.ProfileWire

/**
 * Tiny ViewModel that owns the [KernelBridge] for the gallery's lifetime,
 * drains the kernel's push-callback channel via `nextUpdate`, and
 * republishes the decoded profile slice as a [StateFlow] for Compose.
 *
 * D5/D8 reminder: the kernel is the single source of truth. This class
 * holds NO cached state beyond the latest decoded snapshot. Profile data
 * arrives ONLY via the push callback (mirrors `KernelModel` in
 * `android/app/.../KernelModel.kt`); `nativeGallerySnapshot` returns a
 * status envelope only and is not used here for profile reads.
 *
 * Wire format: every frame is the standard tagged envelope
 *   `{"t":"snapshot","v":{ ..., "profiles": { "<hex>": <ProfileWire>, ... } }}`
 * Anything else (e.g. `t=panic`) is dropped (D7 — actor death is signalled
 * separately and is not a Kotlin concern).
 */
class GalleryModel : ViewModel() {

    private val bridge = KernelBridge()

    private val _profileMap = MutableStateFlow<Map<String, ProfileWire>>(emptyMap())
    val profileMap: StateFlow<Map<String, ProfileWire>> = _profileMap.asStateFlow()

    private val json: Json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    private var pollJob: Job? = null

    init {
        bridge.galleryRegister()
        bridge.start(eventsPerSec = 0, visibleLimit = 80, emitHz = 4)
        startPolling()
        // Claim the demo profile (jack) so the kernel batches a kind:0 fetch.
        bridge.claimProfile(DEMO_PUBKEY, CONSUMER_ID)
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

    /**
     * Pass-through to the kernel action seam; the response JSON envelope is
     * returned verbatim (or null on transport failure).
     */
    fun dispatchAction(action: String, payload: String): String? =
        bridge.dispatchAction(action, payload)

    private fun startPolling() {
        pollJob = viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val raw = bridge.nextUpdate(timeoutMs = 250L) ?: continue
                applyFrame(raw)
            }
        }
    }

    /**
     * Decode one frame from the kernel's push-callback channel. The envelope
     * is `{"t":"snapshot","v":<obj>}`. Non-snapshot frames are dropped; the
     * snapshot body's `profiles` field (if present) merges into the live map.
     */
    private fun applyFrame(raw: String) {
        val outer = runCatching { json.parseToJsonElement(raw).jsonObject }
            .getOrNull() ?: return
        val tag = outer["t"]?.jsonPrimitive?.content
        if (tag != "snapshot") return
        val inner = outer["v"]?.jsonObject ?: return
        val snapshot = runCatching {
            json.decodeFromJsonElement<GallerySnapshot>(inner)
        }.getOrNull() ?: return
        val profiles = snapshot.profiles ?: return
        if (profiles.isEmpty()) return
        _profileMap.value = _profileMap.value + profiles
    }

    override fun onCleared() {
        pollJob?.cancel()
        pollJob = null
        bridge.releaseProfile(DEMO_PUBKEY, CONSUMER_ID)
        bridge.stop()
        bridge.free()
        super.onCleared()
    }

    companion object {
        /** jack — used to demo user-* components against real profile data. */
        const val DEMO_PUBKEY: String =
            "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"

        /** Stable consumer id under which this app holds profile claims. */
        const val CONSUMER_ID: String = "nmp-gallery"
    }
}

/**
 * Minimal decode shape for the snapshot body the gallery cares about.
 * Anything beyond `profiles` is ignored (`Json.ignoreUnknownKeys`).
 *
 * Contract: `profiles` is a JSON object keyed by hex pubkey whose values
 * decode as [ProfileWire]. This matches the kernel-side projection emitted
 * by `nmp-app-gallery` (the Rust crate built in a parallel agent).
 */
@Serializable
private data class GallerySnapshot(
    val profiles: Map<String, ProfileWire>? = null,
)
