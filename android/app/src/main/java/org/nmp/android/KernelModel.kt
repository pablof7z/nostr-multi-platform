package org.nmp.android

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
import org.nmp.android.model.KernelUpdate

/**
 * Observable mirror of the kernel snapshot — the Android peer of iOS
 * `KernelModel`. The Rust actor pushes JSON; a reader coroutine decodes it and
 * republishes via [StateFlow]. Pure mirror: the only guard is `rev` monotonicity
 * (identical to the Swift `guard update.rev > rev` in `apply`). No Kotlin-side
 * business logic or derived state (D5/D8); decode fails closed (D1).
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
                val update = runCatching {
                    json.decodeFromString<KernelUpdate>(payload)
                }.getOrNull() ?: continue          // D1: fail closed, keep prior
                if (update.rev <= _state.value.rev) continue   // mirror only
                withContext(Dispatchers.Main) {
                    _state.value = update
                    _snapshotCount.value += 1
                    _lastSnapshotAtMs.value = System.currentTimeMillis()
                }
            }
        }
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
