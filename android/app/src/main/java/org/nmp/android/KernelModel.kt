package org.nmp.android

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.nmp.android.model.ChirpTimelineSnapshot
import org.nmp.android.model.KernelUpdate

private const val TAG = "NmpCore"

/**
 * Observable mirror of the kernel snapshot — the Android peer of iOS
 * `KernelModel`. The Rust actor pushes JSON; a reader coroutine decodes it and
 * republishes via [StateFlow]. Pure mirror: the only guard is `rev` monotonicity
 * (identical to the Swift `guard update.rev > rev` in `apply`). No Kotlin-side
 * business logic or derived state (D5/D8); decode fails closed (D1).
 *
 * T103 / T107 — wire format: every frame is a tagged envelope
 *   `{"t":"snapshot","v":{…}}` or `{"t":"panic","v":{"msg":…}}`.
 * This model only processes `t=snapshot` frames; the panic arm (D7) is the
 * actor-death terminal signal and is handled separately. Anything else is a
 * wire-format regression and is logged at ERROR.
 */
class KernelModel : ViewModel() {

    private val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
        @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
        namingStrategy = kotlinx.serialization.json.JsonNamingStrategy.SnakeCase
    }

    private val bridge = KernelBridge()

    private val _state = MutableStateFlow(KernelUpdate())
    val state: StateFlow<KernelUpdate> = _state.asStateFlow()

    private val _snapshotCount = MutableStateFlow(0L)
    val snapshotCount: StateFlow<Long> = _snapshotCount.asStateFlow()

    private val _lastSnapshotAtMs = MutableStateFlow<Long?>(null)
    val lastSnapshotAtMs: StateFlow<Long?> = _lastSnapshotAtMs.asStateFlow()

    private var started = false

    fun start() {
        if (started) return
        started = true
        bridge.start(visibleLimit = 80, emitHz = 4)
        viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val payload = bridge.nextUpdate() ?: continue
                val update = decodeSnapshot(payload) ?: continue
                val applied = update.copy(modularTimeline = decodeChirpSnapshot())
                if (applied.rev <= _state.value.rev) continue   // mirror only
                withContext(Dispatchers.Main) {
                    _state.value = applied
                    _snapshotCount.value += 1
                    _lastSnapshotAtMs.value = System.currentTimeMillis()
                }
            }
        }
    }

    fun openTimeline() {
        bridge.openTimeline()
    }

    fun createLocalAccount() {
        bridge.createLocalAccount()
    }

    /**
     * Demand-driven profile fetch claim. Called from a Compose `LaunchedEffect`
     * when a view starts rendering a pubkey; the kernel batches a kind:0 REQ
     * and re-fetches against the author's NIP-65 write set once it lands.
     * Matched by a [releaseProfile] in `DisposableEffect.onDispose`.
     */
    fun claimProfile(pubkey: String, consumerId: String) {
        bridge.claimProfile(pubkey, consumerId)
    }

    /** Inverse of [claimProfile]; safe to call even if no matching claim is live. */
    fun releaseProfile(pubkey: String, consumerId: String) {
        bridge.releaseProfile(pubkey, consumerId)
    }

    /**
     * Decode one frame from the `update_tx` channel.
     *
     * The kernel emits `{"t":"snapshot","v":{…}}` (T103 envelope). Attempt to
     * unwrap the envelope and decode the inner object as [KernelUpdate]. Return
     * null (drop the frame) on any parse error; log enough context to diagnose
     * the failure without flooding logcat (PD-025 finding 4 — no silent swallow).
     *
     * Non-snapshot frames are logged at ERROR and dropped — `t=panic` is the
     * actor-death terminal signal handled separately; anything else is a
     * wire-format regression.
     */
    private fun decodeSnapshot(payload: String): KernelUpdate? {
        // Step 1: parse the outer envelope.
        val outer = runCatching { json.parseToJsonElement(payload).jsonObject }.getOrElse { e ->
            Log.e(TAG, "envelope parse failed: ${e.message}; payload prefix: ${payload.take(200)}")
            return null
        }

        // Step 2: check the discriminator tag.
        val tag = outer["t"]?.jsonPrimitive?.content
        if (tag != "snapshot") {
            Log.e(TAG, "unknown envelope tag=$tag; payload prefix: ${payload.take(200)}")
            return null
        }

        // Step 3: extract the inner snapshot object.
        val inner = outer["v"]?.jsonObject ?: run {
            Log.e(TAG, "snapshot envelope missing 'v' field; payload prefix: ${payload.take(200)}")
            return null
        }

        // Step 4: decode the inner snapshot as KernelUpdate.
        return runCatching {
            json.decodeFromJsonElement<KernelUpdate>(inner)
        }.getOrElse { e ->
            Log.e(TAG, "KernelUpdate decode failed: ${e.message}; inner prefix: ${inner.toString().take(200)}")
            null
        }
    }

    private fun decodeChirpSnapshot(): ChirpTimelineSnapshot {
        val payload = bridge.chirpSnapshot() ?: return ChirpTimelineSnapshot()
        return runCatching {
            json.decodeFromString<ChirpTimelineSnapshot>(payload)
        }.getOrElse { e ->
            Log.e(TAG, "ChirpTimelineSnapshot decode failed: ${e.message}; payload prefix: ${payload.take(200)}")
            ChirpTimelineSnapshot()
        }
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
