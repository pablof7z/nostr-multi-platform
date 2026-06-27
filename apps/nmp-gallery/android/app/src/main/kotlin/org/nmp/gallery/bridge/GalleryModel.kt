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
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
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
 * the push callback only. Registry components resolve pubkeys while visible and
 * resolved profile cards arrive under `projections["refs.profile"]` (ADR-0063
 * #1671 — the resolve_ref output, materialised from the row-delta store in
 * Rust; this host owns no precedence merge).
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
    private val _resolvedEventEmbeds =
        MutableStateFlow<Map<String, ResolvedEventEnvelopeWire>>(emptyMap())
    val resolvedEventEmbeds: StateFlow<Map<String, ResolvedEventEnvelopeWire>> =
        _resolvedEventEmbeds.asStateFlow()

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
    fun resolveProfileRef(pubkey: String, consumerId: String = CONSUMER_ID) {
        bridge.resolveProfileRef(pubkey, consumerId)
    }

    fun releaseProfileRef(pubkey: String, consumerId: String = CONSUMER_ID) {
        bridge.releaseProfileRef(pubkey, consumerId)
    }

    /** App-local URI adapter over the kernel's unified event ref seam. */
    fun resolveEventRef(uri: String, consumerId: String = CONSUMER_ID) {
        bridge.resolveEventRef(uri, consumerId)
    }

    /** Inverse of [resolveEventRef]; safe if the interest is already gone. */
    fun releaseEventRef(uri: String, consumerId: String = CONSUMER_ID) {
        bridge.releaseEventRef(uri, consumerId)
    }

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
     * `projections["refs.profile"]` — ADR-0063 (#1671): the kernel's
     * `refs.profile` row-delta projection (the resolve_ref output), merged
     * host-side across frames in the native session store and materialised
     * under that key by Rust. This host owns no precedence merge.
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

        // ADR-0063 (#1671): projections["refs.profile"][pubkey] is a
        // ProfileWire-shaped entry materialised from the FULL current
        // refs.profile store set (snapshot_json re-materialises the whole store
        // every frame). `npub_short` is derived from `npub` by the ProfileWire
        // constructor default when absent (same algorithm as before).
        //
        // REPLACE the map exactly each frame — no accumulation, no second
        // source of truth (D4). The decoded set IS the live store, so a
        // refs.profile clear/release drops the row here too; unioning would
        // leak released keys.
        (projections["refs.profile"] as? JsonObject)?.let { resolved ->
            for ((pubkey, el) in resolved) {
                val profile = runCatching {
                    json.decodeFromJsonElement<ProfileWire>(el)
                }.getOrNull() ?: continue
                assembled[pubkey] = profile
            }
        }

        _profileMap.value = assembled

        val embeds = mutableMapOf<String, ResolvedEventEnvelopeWire>()
        (projections["refs.event"] as? JsonObject)?.let { resolved ->
            for ((primaryId, el) in resolved) {
                val envelope = runCatching {
                    json.decodeFromJsonElement<ResolvedEventEnvelopeWire>(el)
                }.getOrNull() ?: continue
                embeds[primaryId] = envelope
            }
        }
        _resolvedEventEmbeds.value = embeds

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
data class ResolvedEventEnvelopeWire(
    @SerialName("uri") val uri: String = "",
    @SerialName("primary_id") val primaryId: String = "",
    @SerialName("depth") val depth: Int = 0,
    @SerialName("max_depth") val maxDepth: Int = 4,
    @SerialName("collapsed") val collapsed: Boolean = false,
    @SerialName("collapse_reason") val collapseReason: String? = null,
    @SerialName("projection") val projection: JsonObject = JsonObject(emptyMap()),
)

val ResolvedEventEnvelopeWire.projectionVariant: String
    get() = projection["variant"]?.jsonPrimitive?.contentOrNull.orEmpty()

fun ResolvedEventEnvelopeWire.projectionString(key: String): String? =
    projectionData()[key]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotEmpty() }

fun ResolvedEventEnvelopeWire.projectionLong(key: String): Long? =
    projectionData()[key]?.jsonPrimitive?.longOrNull

fun ResolvedEventEnvelopeWire.projectionStrings(key: String): List<String> =
    (projectionData()[key] as? JsonArray)
        ?.mapNotNull { it.jsonPrimitive.contentOrNull?.takeIf(String::isNotEmpty) }
        ?: emptyList()

fun ResolvedEventEnvelopeWire.projectionContentText(): String? =
    contentTreeText(projectionData()["contentTree"]?.jsonObject)

private fun ResolvedEventEnvelopeWire.projectionData(): JsonObject =
    projection["data"]?.jsonObject ?: JsonObject(emptyMap<String, JsonElement>())

private fun contentTreeText(tree: JsonObject?): String? {
    val nodes = tree?.get("nodes")?.jsonArray ?: return null
    val roots = tree["roots"]?.jsonArray ?: return null
    val text = roots
        .mapNotNull { it.jsonPrimitive.intOrNull }
        .joinToString(" ") { renderContentNode(it, nodes, emptySet()) }
        .trim()
    return text.takeIf { it.isNotEmpty() }
}

private fun renderContentNode(index: Int, nodes: JsonArray, seen: Set<Int>): String {
    if (index in seen) return ""
    val node = nodes.getOrNull(index)?.jsonObject ?: return ""
    val nextSeen = seen + index
    fun childrenText(key: String = "children"): String =
        node[key]
            ?.jsonArray
            ?.mapNotNull { it.jsonPrimitive.intOrNull }
            ?.joinToString("") { renderContentNode(it, nodes, nextSeen) }
            .orEmpty()
    return when (node["kind"]?.jsonPrimitive?.contentOrNull) {
        "text" -> node["text"]?.jsonPrimitive?.contentOrNull.orEmpty()
        "url" -> node["url"]?.jsonPrimitive?.contentOrNull.orEmpty()
        "hashtag" -> node["tag"]?.jsonPrimitive?.contentOrNull?.let { "#$it" }.orEmpty()
        "inline_code" -> node["code"]?.jsonPrimitive?.contentOrNull.orEmpty()
        "soft_break" -> " "
        "hard_break" -> "\n"
        "paragraph", "heading", "emphasis", "strong", "block_quote", "link" -> childrenText()
        "list" -> node["items"]
            ?.jsonArray
            ?.joinToString("\n") { item ->
                item.jsonArray
                    .mapNotNull { it.jsonPrimitive.intOrNull }
                    .joinToString("") { renderContentNode(it, nodes, nextSeen) }
            }
            .orEmpty()
        "code_block" -> node["body"]?.jsonPrimitive?.contentOrNull.orEmpty()
        "image" -> node["alt"]?.jsonPrimitive?.contentOrNull.orEmpty()
        "mention", "event_ref" -> node["uri"]
            ?.jsonObject
            ?.get("primary_id")
            ?.jsonPrimitive
            ?.contentOrNull
            .orEmpty()
        else -> ""
    }
}
