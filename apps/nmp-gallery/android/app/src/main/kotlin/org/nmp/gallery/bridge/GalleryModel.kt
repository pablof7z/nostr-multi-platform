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
import kotlinx.serialization.json.jsonPrimitive
import org.nmp.gallery.registry.ProfileWire

/**
 * Tiny ViewModel that owns the [KernelBridge] for the gallery's lifetime,
 * drains the kernel's FlatBuffers push-callback channel via `nextUpdate`, and
 * republishes the decoded profile slice as a [StateFlow] for Compose.
 *
 * D5/D8: the kernel is the single source of truth. Profile data arrives via
 * the push callback only. Registry components claim pubkeys while visible and
 * claimed profile cards arrive in `projections.claimed_profiles`.
 */
class GalleryModel : ViewModel() {

    private val bridge = KernelBridge()
    val showcase: GalleryShowcaseReferences =
        GalleryShowcaseReferences.decode(bridge.showcaseReferencesJson())

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
                    bridge.nextUpdate(timeoutMs = 250L)
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
     * Decode one FlatBuffers snapshot frame. Profiles are assembled from
     * component-owned `projections.claimed_profiles`, plus the author/mention
     * projections used by other gallery showcases.
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

        // Component-owned path: projections.claimed_profiles[pubkey].
        (projections["claimed_profiles"] as? JsonObject)?.let { claimed ->
            for ((pubkey, el) in claimed) {
                val card = runCatching {
                    json.decodeFromJsonElement<ProfileCard>(el)
                }.getOrNull() ?: continue
                assembled[pubkey] = card.toProfileWire(pubkey)
            }
        }

        // Author-view fallback: projections.author_view.profile (ProfileCard shape)
        (projections["author_view"] as? JsonObject)?.let { av ->
            val pubkey = av["pubkey"]?.jsonPrimitive?.content ?: return@let
            val profileEl = av["profile"] as? JsonObject ?: return@let
            val card = runCatching {
                json.decodeFromJsonElement<ProfileCard>(profileEl)
            }.getOrNull() ?: return@let
            assembled[pubkey] = card.toProfileWire(pubkey)
        }

        // Secondary: projections.mention_profiles (display_name + picture_url only)
        (projections["mention_profiles"] as? JsonObject)?.let { mentions ->
            for ((pubkey, el) in mentions) {
                if (assembled.containsKey(pubkey)) continue
                val mp = runCatching {
                    json.decodeFromJsonElement<MentionProfilePayload>(el)
                }.getOrNull() ?: continue
                assembled[pubkey] = ProfileWire(
                    pubkey = pubkey,
                    displayName = mp.displayName,
                    pictureUrl = mp.pictureUrl,
                    npub = "",
                    npubShort = pubkey.take(8) + "…" + pubkey.takeLast(8),
                )
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

/**
 * Wire shape of `projections.author_view.profile` — the kernel's ProfileCard.
 * `npub_short` is not emitted by the kernel; we derive it from `npub`.
 */
@Serializable
private data class ProfileCard(
    @SerialName("pubkey") val pubkey: String = "",
    @SerialName("npub") val npub: String = "",
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("picture_url") val pictureUrl: String? = null,
    @SerialName("nip05") val nip05: String? = null,
    @SerialName("about") val about: String? = null,
) {
    fun toProfileWire(overridePubkey: String): ProfileWire {
        val pk = overridePubkey.ifEmpty { pubkey }
        val short = if (npub.length > 16) npub.take(8) + "…" + npub.takeLast(8) else npub
        return ProfileWire(
            pubkey = pk,
            displayName = displayName?.takeIf { it.isNotEmpty() },
            about = about?.takeIf { it.isNotEmpty() },
            pictureUrl = pictureUrl?.takeIf { it.isNotEmpty() },
            nip05 = nip05?.takeIf { it.isNotEmpty() },
            npub = npub,
            npubShort = short,
        )
    }
}

@Serializable
private data class MentionProfilePayload(
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("picture_url") val pictureUrl: String? = null,
)
