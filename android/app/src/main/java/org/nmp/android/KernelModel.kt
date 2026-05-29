package org.nmp.android

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.nmp.android.model.AccountSummary
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.KernelUpdate
import org.nmp.android.model.RelayStatus

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

    /** Derived: account list from the latest snapshot projections. */
    val accounts: StateFlow<List<AccountSummary>> =
        state.map { it.projections?.accounts ?: emptyList() }
            .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    /** Derived: relay status list from the latest snapshot. */
    val relays: StateFlow<List<RelayStatus>> =
        state.map { it.relayStatuses }
            .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

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
     * Publish a new note. Routes through dispatch_action("nmp.publish", ...).
     *
     * Returns the correlation_id if accepted, or null on error.
     */
    fun publishNote(
        content: String,
        replyToId: String? = null,
        target: String = "Auto",
    ): String? {
        val actionJson = when {
            replyToId != null -> {
                """{"PublishNote":{"content":"${escapeJson(content)}","reply_to_id":"$replyToId","target":"$target"}}"""
            }
            else -> {
                """{"PublishNote":{"content":"${escapeJson(content)}","reply_to_id":null,"target":"$target"}}"""
            }
        }
        val response = bridge.dispatchAction("nmp.publish", actionJson)
        return try {
            val json = org.json.JSONObject(response)
            json.optString("correlation_id").takeIf { it.isNotEmpty() }
        } catch (e: Exception) {
            Log.d(TAG, "publishNote parse error: $response", e)
            null
        }
    }

    /**
     * Open a thread by note ID. The kernel batches a kind:1 REQ and opens
     * the timeline to display the thread.
     */
    fun openThread(noteId: String) {
        bridge.openThread(noteId)
    }

    /**
     * Open an author profile by pubkey. The kernel batches a kind:0 REQ and
     * opens the timeline to display the author's notes.
     */
    fun openAuthor(pubkey: String) {
        bridge.openAuthor(pubkey)
    }

    /**
     * Dispatch a named action through the action registry (generic path).
     * Fire-and-forget — outcomes arrive in the next snapshot tick.
     */
    fun dispatchAction(namespace: String, actionJson: String) {
        val response = bridge.dispatchAction(namespace, actionJson)
        Log.d(TAG, "dispatchAction($namespace) response: $response")
    }

    // -------------------------------------------------------------------------
    // Account management
    // -------------------------------------------------------------------------

    /** Sign in with an nsec secret key. */
    fun signInNsec(secret: String) {
        bridge.dispatchAction("nmp.sign_in_nsec", """{"SignInNsec":{"secret":"$secret"}}""")
        bridge.openTimeline()
    }

    /** Create a new local account with the given display name. */
    fun createAccount(displayName: String) {
        bridge.createLocalAccount(displayName)
        // Mirror desktop bridge: openTimeline after account creation so the
        // kernel starts fetching notes for the new account immediately.
        bridge.openTimeline()
    }

    /** Switch the active account to the given pubkey. */
    fun switchAccount(pubkey: String) {
        bridge.dispatchAction("nmp.switch_account", """{"pubkey":"$pubkey"}""")
        bridge.openTimeline()
    }

    /** Remove the account identified by the given pubkey. */
    fun removeAccount(pubkey: String) = bridge.dispatchAction("nmp.remove_account", """{"pubkey":"$pubkey"}""")

    // -------------------------------------------------------------------------
    // Relay management
    // -------------------------------------------------------------------------

    /** Add a relay with the given URL and role ("read", "write", or "both"). */
    fun addRelay(url: String, role: String = "both") = bridge.dispatchAction("nmp.add_relay", """{"url":"$url","role":"$role"}""")

    /** Remove a relay by URL. */
    fun removeRelay(url: String) = bridge.dispatchAction("nmp.remove_relay", """{"url":"$url"}""")

    // -------------------------------------------------------------------------
    // Social
    // -------------------------------------------------------------------------

    /** Zap a note (NIP-57). */
    fun zapNote(eventId: String, recipientPubkey: String, amountMsats: Long = 21000L, comment: String = "") =
        bridge.dispatchAction("nmp.nip57.zap", """{"target_event_id":"$eventId","recipient_pubkey":"$recipientPubkey","amount_msats":$amountMsats,"comment":"${escapeJson(comment)}"}""")

    /** React to a note (NIP-25). */
    fun react(eventId: String, reaction: String = "+") =
        bridge.dispatchAction("nmp.nip25.react", """{"target_event_id":"$eventId","reaction":"$reaction"}""")

    /** Follow a pubkey. */
    fun follow(pubkey: String) = bridge.dispatchAction("nmp.follow", """{"pubkey":"$pubkey"}""")

    /** Unfollow a pubkey. */
    fun unfollow(pubkey: String) = bridge.dispatchAction("nmp.unfollow", """{"pubkey":"$pubkey"}""")

    // -------------------------------------------------------------------------
    // DMs
    // -------------------------------------------------------------------------

    /** Send a NIP-17 direct message to the given recipient pubkey. */
    fun sendDm(recipientPubkey: String, content: String) =
        bridge.dispatchAction("nmp.nip17.send", """{"recipient_pubkey":"$recipientPubkey","content":"${escapeJson(content)}"}""")

    // -------------------------------------------------------------------------
    // Wallet (NIP-47 / NWC)
    // -------------------------------------------------------------------------

    /**
     * Connect a NIP-47 wallet via NWC URI. Routes through dispatch_action("nmp.wallet.connect", ...).
     *
     * The actionJson format is: {"Connect":{"uri":"nostr+walletconnect://..."}}
     */
    fun dispatchWalletConnect(actionJson: String) {
        val response = bridge.dispatchAction("nmp.wallet.connect", actionJson)
        Log.d(TAG, "wallet connect response: $response")
    }

    /**
     * Disconnect the current NIP-47 wallet. Routes through dispatch_action("nmp.wallet.disconnect", ...).
     */
    fun dispatchWalletDisconnect() {
        val response = bridge.dispatchAction("nmp.wallet.disconnect", "\"Disconnect\"")
        Log.d(TAG, "wallet disconnect response: $response")
    }

    private fun escapeJson(s: String): String {
        return s.replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
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
                // ADR-0038 V-85: the typed `nmp.feed.home` NOFS decoder
                // ([TypedHomeFeedDecoder]) now fills `contentTree` via the
                // native Kotlin NFCT decoder, making the typed path
                // render-complete. Prefer it when present; fall back to the
                // generic `Value` projection (ADR-0037 Commitment 4: the
                // generic path is a permanent fallback, never removed).
                val typed: ChirpOpFeedSnapshot? =
                    TypedHomeFeedDecoder.decode(frame.typedProjections)
                if (typed != null) frame.update.copy(modularTimeline = typed) else frame.update
            }
        }
    }

    override fun onCleared() {
        bridge.stop()
        bridge.free()
        super.onCleared()
    }
}
