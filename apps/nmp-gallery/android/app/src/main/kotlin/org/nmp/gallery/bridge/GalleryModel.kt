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
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import org.nmp.gallery.gallery.REGISTRY_SECTIONS
import org.nmp.gallery.gallery.RegistrySection
import org.nmp.gallery.gallery.parseRegistryJson
import org.nmp.gallery.registry.ProfileWire

/**
 * Tiny ViewModel that owns the [KernelBridge] for the gallery's lifetime,
 * drains the kernel's FlatBuffers push-callback channel via `nextUpdate`, and
 * republishes the decoded profile slice as a [StateFlow] for Compose.
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

    private val json: Json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    private var pollJob: Job? = null

    init {
        bridge.galleryRegister()
        bridge.start(eventsPerSec = 0, visibleLimit = 80, emitHz = 4)
        startPolling()
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

    private fun startPolling() {
        pollJob = viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val raw = try {
                    // Block until a snapshot arrives (D8 — no polling).
                    // Kernel emits at ~4 Hz; 30s timeout is defensive.
                    bridge.nextUpdate(timeoutMs = 30_000L)
                } catch (e: IllegalStateException) {
                    // V-57 P5: Rust JNI distinguishes RecvTimeoutError::Disconnected
                    // (channel closed — sender dropped) from RecvTimeoutError::Timeout
                    // (idle tick — keep polling). A disconnect surfaces as this
                    // exception. Break out of the loop instead of spinning on a
                    // dead channel.
                    android.util.Log.i("GalleryModel", "snapshot channel closed: ${e.message}")
                    break
                } ?: continue
                applyFrame(raw)
            }
        }
    }

    /**
     * Decode one FlatBuffers snapshot frame. Profiles are read directly from
     * `projections.resolved_profiles` — the kernel's single, pre-merged profile
     * projection (added in PR #812). The three-source merge (claimed_profiles +
     * author_view.profile + mention_profiles, with its precedence rule) now
     * lives in the kernel, so this host no longer reimplements it.
     */
    private fun applyFrame(raw: ByteArray) {
        val v = try {
            NmpUpdateFrameDecoder.decodeSnapshot(raw)
        } catch (e: UpdateFrameDecodeException) {
            android.util.Log.w("GalleryModel", "drop frame: ${e.message}")
            return
        }
        val projections = (v["projections"] as? JsonObject) ?: return

        val assembled = mutableMapOf<String, ProfileWire>()

        // Kernel-merged path: projections.resolved_profiles[pubkey] is already
        // a ProfileWire-shaped entry. `npub_short` is derived from `npub` by the
        // ProfileWire constructor default when absent (same algorithm as before).
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
    }

    override fun onCleared() {
        pollJob?.cancel()
        pollJob = null
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
