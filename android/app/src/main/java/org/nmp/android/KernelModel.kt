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
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.nmp.android.model.KernelUpdate

private const val TAG = "NmpCore"

/**
 * Observable mirror of the kernel snapshot — the Android peer of iOS
 * `KernelModel`. The Rust actor pushes JSON; a reader coroutine decodes it and
 * republishes via [StateFlow]. Pure mirror: the only guard is `rev` monotonicity
 * (identical to the Swift `guard update.rev > rev` in `apply`). No Kotlin-side
 * business logic or derived state (D5/D8); decode fails closed (D1).
 *
 * T103 / T107 — wire format: every frame is a tagged envelope. This model
 * processes `t=full_state` frames (`t=snapshot` is accepted as a legacy alias);
 * non-state frames are intentionally ignored because the full-state payload
 * already carries the projected UI state.
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
                if (update.rev <= _state.value.rev) continue   // mirror only
                withContext(Dispatchers.Main) {
                    _state.value = update
                    _snapshotCount.value += 1
                    _lastSnapshotAtMs.value = System.currentTimeMillis()
                }
            }
        }
    }

    /**
     * Decode one frame from the `update_tx` channel.
     *
     * The kernel emits `{"t":"full_state","v":{…}}` (T103 envelope). Attempt to
     * unwrap the envelope and decode the inner object as [KernelUpdate]. Return
     * null (drop the frame) on any parse error; log enough context to diagnose
     * the failure without flooding logcat (PD-025 finding 4 — no silent swallow).
     *
     * Non-state frames are logged at DEBUG and dropped — the full-state
     * projection already carries the full UI state.
     */
    private fun decodeSnapshot(payload: String): KernelUpdate? {
        // Step 1: parse the outer envelope.
        val outer = runCatching { json.parseToJsonElement(payload).jsonObject }.getOrElse { e ->
            Log.e(TAG, "envelope parse failed: ${e.message}; payload prefix: ${payload.take(200)}")
            return null
        }

        // Step 2: check the discriminator tag.
        val tag = outer["t"]?.jsonPrimitive?.content
        if (tag != "full_state" && tag != "snapshot") {
            if (tag == "update" || tag == "side_effect") {
                Log.d(TAG, "non-state frame received (ignored by full-state model)")
            } else {
                Log.e(TAG, "unknown envelope tag=$tag; payload prefix: ${payload.take(200)}")
            }
            return null
        }

        // Step 3: extract the inner full-state object.
        val inner = outer["v"]?.jsonObject ?: run {
            Log.e(TAG, "full_state envelope missing 'v' field; payload prefix: ${payload.take(200)}")
            return null
        }

        // Step 4: decode the inner full-state payload as KernelUpdate.
        return runCatching {
            json.decodeFromJsonElement<KernelUpdate>(inner)
        }.getOrElse { e ->
            Log.e(TAG, "KernelUpdate decode failed: ${e.message}; inner prefix: ${inner.toString().take(200)}")
            null
        }
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
