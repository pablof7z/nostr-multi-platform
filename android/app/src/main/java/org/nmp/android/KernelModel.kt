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
import org.nmp.android.model.ChirpTimelineSnapshot
import org.nmp.android.model.KernelUpdate

private const val TAG = "NmpCore"

/**
 * Observable mirror of the kernel snapshot — the Android peer of iOS
 * `KernelModel`. The Rust actor pushes FlatBuffers `UpdateFrame` bytes
 * (file_identifier "NMPU"); a reader coroutine decodes them via
 * [KernelUpdateFrameDecoder] and republishes via [StateFlow].
 *
 * Pure mirror: the only guard is `rev` monotonicity (identical to the Swift
 * `guard update.rev > rev` in `apply`). No Kotlin-side business logic or
 * derived state (D5/D8); decode fails closed (D1).
 *
 * Each [ByteArray] from `nextUpdate()` carries both the generic [KernelUpdate]
 * (decoded from the `SnapshotFrame.payload` `Value` tree) AND the typed
 * `nmp.feed.home` FlatBuffers projection (file_identifier "NFTS") embedded in
 * `SnapshotFrame.typed_projections`. Both are extracted in a single pass
 * through [KernelUpdateFrameDecoder.decode] — no second FFI call needed.
 */
class KernelModel : ViewModel() {

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
                val bytes = try {
                    bridge.nextUpdate()
                } catch (e: IllegalStateException) {
                    // Mirrors PR #644 / V-57 P5 for nmp-gallery: the Rust JNI
                    // distinguishes RecvTimeoutError::Disconnected (channel
                    // closed — sender dropped) from RecvTimeoutError::Timeout
                    // (idle tick — keep polling). A disconnect surfaces as
                    // this exception. Break out of the loop instead of
                    // spinning on a dead channel.
                    Log.i(TAG, "update channel closed: ${e.message}")
                    break
                } ?: continue

                val decoded = decodeUpdate(bytes) ?: continue
                if (decoded.rev <= _state.value.rev) continue  // mirror only
                withContext(Dispatchers.Main) {
                    _state.value = decoded
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
     * Decode one FlatBuffers update frame.
     *
     * Extracts both the generic [KernelUpdate] (from `SnapshotFrame.payload`)
     * and the typed `nmp.feed.home` timeline projection (from
     * `SnapshotFrame.typed_projections`) in a single pass.  Returns `null`
     * (drop the frame) on any parse error; logs enough context to diagnose
     * the failure without flooding logcat (PD-025 finding 4 — no silent
     * swallow).
     *
     * Panic frames are logged at ASSERT level — they indicate actor death (D7)
     * and must not be silently ignored, but Android has no way to propagate
     * them to a UI toast from a background coroutine without additional
     * infrastructure. Future work: surface via a dedicated `panicState` flow.
     */
    private fun decodeUpdate(bytes: ByteArray): KernelUpdate? {
        return when (val frame = KernelUpdateFrameDecoder.decode(bytes)) {
            null -> null
            is KernelDecodedUpdateFrame.Panic -> {
                Log.wtf(TAG, "NMP_ACTOR_PANIC: ${frame.message}")
                null
            }
            is KernelDecodedUpdateFrame.Snapshot -> {
                // ADR-0038 Stage T4: the typed `nmp.feed.home` decoder
                // ([TypedHomeFeedDecoder]) now targets the OP-centric `NOFS`
                // shape and returns the distinct `ChirpOpFeedSnapshot` type. It
                // is intentionally NOT wired into the render preference here
                // (decoder-only, matching the iOS T3 posture): Android has no
                // Kotlin `NFCT` decoder, so the typed card's content tree cannot
                // be filled and a typed render would show blank content. Until
                // that follow-up lands the host always renders the generic
                // `Value` projection carried in `frame.update.modularTimeline`.
                // (The prior NFTS wiring overwrote the generic timeline with an
                // empty NFTS decode anyway — the producer emits `NOFS`, not
                // `NFTS`, since Stage T1 — so dropping it also fixes a latent
                // feed-blanking bug.)
                frame.update
            }
        }
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
