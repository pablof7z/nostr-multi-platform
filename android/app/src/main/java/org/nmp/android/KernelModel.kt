package org.nmp.android

import android.content.Context
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import org.nmp.android.model.AccountSummary
import org.nmp.android.model.SignerState
import org.nmp.android.model.ChirpOpFeedSnapshot
import org.nmp.android.model.KernelUpdate
import org.nmp.android.model.RelayStatus
import org.nmp.android.model.TimelineWindowCursor

private const val TAG = "NmpCore"
private const val HOME_FEED_KEY = "nmp.feed.home"

/**
 * Observable mirror of the kernel snapshot — Android peer of iOS `KernelModel`.
 * Rust pushes FlatBuffers `UpdateFrame` bytes (file_identifier "NMPU"); the
 * reader coroutine decodes them via [KernelUpdateFrameDecoder] and republishes
 * via [StateFlow]. Pure mirror: rev-monotonicity guard only (D5/D8). Fails
 * closed (D1). `payload:Value` removed in PR #1082; typed-sidecar path is
 * authoritative. Marmot write ops live in [MarmotActions] (see [marmot]).
 */
class KernelModel : ViewModel() {

    private val bridge = KernelBridge()

    /**
     * Marmot (MLS-over-Nostr encrypted groups) write operations. Mirrors the iOS
     * `model.marmot` surface; all UI call sites use `model.marmot.<op>()`.
     * Extracted into [MarmotActions] to keep this file under the 500-LOC ceiling.
     */
    val marmot = MarmotActions(dispatchAction = { ns, json -> bridge.dispatchAction(ns, json) })

    private val _state = MutableStateFlow(KernelUpdate())
    val state: StateFlow<KernelUpdate> = _state.asStateFlow()

    private val _snapshotCount = MutableStateFlow(0L)
    val snapshotCount: StateFlow<Long> = _snapshotCount.asStateFlow()

    private val _lastSnapshotAtMs = MutableStateFlow<Long?>(null)
    val lastSnapshotAtMs: StateFlow<Long?> = _lastSnapshotAtMs.asStateFlow()

    private val _kernelIsDead = MutableStateFlow(false)
    val kernelIsDead: StateFlow<Boolean> = _kernelIsDead.asStateFlow()

    /** Derived: account list from the latest snapshot projections. */
    val accounts: StateFlow<List<AccountSummary>> =
        state.map { it.projections?.accounts ?: emptyList() }
            .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    /** Derived: relay status list from the latest snapshot. */
    val relays: StateFlow<List<RelayStatus>> =
        state.map { it.relayStatuses }
            .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    /** ADR-0048 D6: unified remote-signer health (NIP-46 + NIP-55). Null = local
     *  key (steady state). `isReady` = green, `isAwaitingApproval`/`isReconnecting`
     *  = amber, `isUnavailable`/`isFailed` = red. Drives `SignerStateRow`. */
    val signerState: StateFlow<SignerState?> =
        state.map { it.projections?.signerState }
            .stateIn(viewModelScope, SharingStarted.Eagerly, null)

    private var started = false
    private var signerReaderJob: Job? = null
    private var lastLoadMoreCursor: TimelineWindowCursor? = null

    /** ADR-0048 Stage 2 — NIP-55 capability handler (activity-registered).
     *  Null = no activity registered; requests degrade to Rust-side timeout (D6). */
    @Volatile
    private var externalSignerHandler: ((requestJson: String) -> Unit)? = null

    /** Register the activity-owned NIP-55 request handler. */
    fun registerExternalSignerHandler(handler: (requestJson: String) -> Unit) {
        externalSignerHandler = handler
    }

    /** Unregister on activity teardown (the launcher is being released). */
    fun unregisterExternalSignerHandler() {
        externalSignerHandler = null
    }

    /**
     * Android Keystore keyring capability handler. Registered synchronously
     * before [start] calls identity-restore so the keyring is live when the
     * kernel reads the persisted nsec.
     *
     * Initialised lazily on the first [start] call so the [Context] is not
     * required at ViewModel construction time. Callers must invoke
     * [startWithContext] instead of [start] when identity-restore is needed.
     */
    @Volatile
    private var keystoreKeyringCapability: KeystoreKeyringCapability? = null

    /**
     * Start the kernel with a storage path and an Android [Context] for the
     * Keystore keyring capability. Registers the keyring handler BEFORE
     * calling identity-restore so the persisted secret can be retrieved on
     * cold start. Prefer this over the context-free [start] when the app
     * manages sign-in state across restarts.
     *
     * [testNsec] is null in production; pass only in headless UI tests.
     * [testRelays] is null in production; when non-null it must be a JSON
     * array of `["url","role"]` pairs that REPLACES the Chirp reference
     * relays — used by E2E test harnesses to point the kernel at a local
     * relay (e.g. `nak serve`). Kotlin ferries this string verbatim; all
     * parsing and policy live in Rust (D7 / thin-shell principle).
     */
    fun startWithContext(
        context: Context,
        storagePath: String? = null,
        testNsec: String? = null,
        testRelays: String? = null,
    ) {
        if (started) return
        keystoreKeyringCapability = KeystoreKeyringCapability(context.applicationContext)
        bridge.setCapabilityHandler(keystoreKeyringCapability!!)
        start(storagePath, testRelays)
        bridge.identityRestore(
            dbDir = storagePath ?: context.filesDir.path,
            testNsec = testNsec,
        )
    }

    fun start(storagePath: String? = null, testRelays: String? = null) {
        if (started) return
        started = true
        if (!storagePath.isNullOrBlank()) {
            bridge.setStoragePath(storagePath)
        }
        bridge.start(visibleLimit = 80, emitHz = 4)
        bridge.seedRelays(testRelays)
        // Issue #614 — push model (D8 no-polling): the kernel invokes this
        // callback on its update-listener thread for every frame, replacing the
        // former blocking-drain coroutine. The kernel is the single writer, so
        // the rev-monotonicity read/write below stays correct. `MutableStateFlow`
        // value assignment is thread-safe; UI collectors observe on their own
        // dispatcher.
        bridge.setUpdateListener { bytes ->
            val decoded = decodeUpdate(bytes) ?: return@setUpdateListener
            if (decoded.rev <= _state.value.rev) return@setUpdateListener  // mirror only
            _state.value = decoded
            _snapshotCount.value += 1
            _lastSnapshotAtMs.value = System.currentTimeMillis()
        }
        // ADR-0048 Stage 2 — drain NIP-55 requests; dispatch on Main (Intent requires it).
        signerReaderJob = viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val requestJson = try {
                    bridge.nextSignerRequest()
                } catch (e: IllegalStateException) {
                    Log.i(TAG, "signer request channel closed: ${e.message}")
                    break
                } ?: continue
                val handler = externalSignerHandler
                if (handler == null) {
                    Log.w(TAG, "NIP-55 request dropped: no capability bridge registered")
                    continue
                }
                withContext(Dispatchers.Main) { handler(requestJson) }
            }
        }
    }

    fun openTimeline() {
        bridge.openTimeline()
    }

    /** Report Android lifecycle foreground to Rust. */
    fun lifecycleForeground() {
        bridge.lifecycleForeground()
    }

    /** Report Android lifecycle background to Rust. */
    fun lifecycleBackground() {
        bridge.lifecycleBackground()
    }

    /**
     * Pull-side actor-liveness probe for foreground resume. If Android missed
     * the pushed panic frame while backgrounded, this still latches the fatal
     * kernel state for the host.
     */
    fun checkAlive() {
        if (!bridge.isAlive()) {
            _kernelIsDead.value = true
        }
    }

    fun createLocalAccount() {
        bridge.createLocalAccount()
    }

    /** Demand-driven profile fetch claim (Compose LaunchedEffect → DisposableEffect). */
    fun claimProfile(pubkey: String, consumerId: String) {
        bridge.claimProfile(pubkey, consumerId)
    }

    /** Inverse of [claimProfile]; safe to call even if no matching claim is live. */
    fun releaseProfile(pubkey: String, consumerId: String) {
        bridge.releaseProfile(pubkey, consumerId)
    }

    /** NIP-19 display identifier (nprofile1… or npub1…). ADR-0032 / V-115. */
    fun encodeProfile(pubkey: String): String? = bridge.encodeProfile(pubkey)

    /**
     * Publish a new note. Kotlin forwards only user intent; Rust builds the
     * `nmp.publish` namespace and `PublishRaw` body, including reply tags.
     *
     * Returns the correlation_id if accepted, or null on error.
     */
    fun publishNote(
        content: String,
        replyToId: String? = null,
    ): String? {
        val response = dispatchTypedIntent(
            ChirpActionIntent(
                type = "publish_note",
                content = content,
                replyToEventId = replyToId,
            )
        ) ?: return null
        return response.correlationId
    }

    /** Extend home-feed window; [after] is an opaque edge cursor (Rust owns page policy). */
    fun loadOlderTimeline(after: TimelineWindowCursor) {
        val page = state.value.modularTimeline.page ?: return
        if (!page.hasMore) return
        if (lastLoadMoreCursor == after) return
        lastLoadMoreCursor = after
        bridge.loadOlderFeed(HOME_FEED_KEY)
    }

    /**
     * Open a thread by note ID. Rust registers `nmp.feed.thread.<noteId>`.
     */
    fun openThread(noteId: String) {
        bridge.openThread(noteId)
    }

    /** Close the dynamic thread feed opened by [openThread]. */
    fun closeThread(noteId: String) {
        bridge.closeThread(noteId)
    }

    /**
     * Open an author profile by pubkey. Rust registers `nmp.feed.author.<pubkey>`.
     */
    fun openAuthor(pubkey: String) {
        bridge.openAuthor(pubkey)
    }

    /** Close the dynamic author feed opened by [openAuthor]. */
    fun closeAuthor(pubkey: String) {
        bridge.closeAuthor(pubkey)
    }

    /**
     * Dispatch a named action through the action registry (generic path).
     * Fire-and-forget — outcomes arrive in the next snapshot tick.
     */
    fun dispatchAction(namespace: String, actionJson: String): DispatchResult {
        val result = bridge.dispatchAction(namespace, actionJson)
        Log.d(TAG, "dispatchAction($namespace) response: $result")
        return result
    }

    fun ackActionStage(correlationId: String) {
        bridge.ackActionStage(correlationId)
    }

    // -------------------------------------------------------------------------
    // Account management
    // -------------------------------------------------------------------------

    /** Sign in with an nsec secret key (direct C-ABI — no ActionModule for sign-in namespace). */
    fun signInNsec(secret: String) {
        bridge.signInNsec(secret)
        bridge.openTimeline()
    }

    /** Sign in with a NIP-46 bunker URI through the Rust signer broker. */
    fun signInBunker(uri: String) {
        bridge.signInBunker(uri)
    }

    /** ADR-0048 Stage 2 — begin NIP-55 sign-in; Rust builds the get_public_key request. */
    fun signInWithAmber(signer: NostrSignerInfo) {
        bridge.signInNip55(signer.packageName ?: signer.contentAuthority)
    }

    /** ADR-0048 Stage 2 — route ExternalSignerResponse JSON back to the Rust NIP-55 driver. */
    fun deliverSignerResponse(responseJson: String) {
        bridge.deliverSignerResponse(responseJson)
    }

    fun cancelBunkerHandshake() {
        bridge.cancelBunkerHandshake()
    }

    fun nostrConnectUri(relayUrl: String? = null, callbackScheme: String? = null): String? =
        bridge.nostrConnectUri(relayUrl, callbackScheme)

    /** Create a new local account with the given display name. */
    fun createAccount(displayName: String) {
        bridge.createLocalAccount(displayName)
        bridge.openTimeline()
    }

    /** Switch the active account (direct C-ABI — no ActionModule for switch namespace). */
    fun switchAccount(pubkey: String) {
        bridge.switchAccount(pubkey)
        bridge.openTimeline()
    }

    /** Remove the account identified by the given pubkey (direct C-ABI). */
    fun removeAccount(pubkey: String) = bridge.removeAccount(pubkey)

    // -------------------------------------------------------------------------
    // Relay management
    // -------------------------------------------------------------------------

    /** Add a relay with the given URL and role ("read", "write", or "both"). */
    fun addRelay(url: String, role: String = "both") = bridge.addRelay(url, role)

    /** Remove a relay by URL. */
    fun removeRelay(url: String) = bridge.removeRelay(url)

    // -------------------------------------------------------------------------
    // Social
    // -------------------------------------------------------------------------

    /** Zap a note (NIP-57). */
    fun zapNote(
        eventId: String,
        recipientPubkey: String,
        amountMsats: Long = 21000L,
        comment: String = "",
    ): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(
            type = "zap",
            targetEventId = eventId,
            recipientPubkey = recipientPubkey,
            amountMsats = amountMsats,
            comment = comment.takeIf { it.isNotEmpty() },
        )
    )

    /** React to a note (NIP-25). */
    fun react(eventId: String, reaction: String = "+"): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "react", eventId = eventId, reaction = reaction)
    )

    /** Follow a pubkey. */
    fun follow(pubkey: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "follow", pubkey = pubkey)
    )

    /** Unfollow a pubkey. */
    fun unfollow(pubkey: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "unfollow", pubkey = pubkey)
    )

    // -------------------------------------------------------------------------
    // DMs
    // -------------------------------------------------------------------------

    /** Send a NIP-17 direct message to the given recipient pubkey. */
    fun sendDm(recipientPubkey: String, content: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "send_dm", recipientPubkey = recipientPubkey, content = content)
    )

    // -------------------------------------------------------------------------
    // Marmot registration trampoline — write ops live in [marmot: MarmotActions]
    // -------------------------------------------------------------------------

    /** Idempotent per-account Marmot MLS registration. [dbDir] = context.filesDir.path. */
    fun registerMarmotIfNeeded(dbDir: String) {
        marmot.registerIfNeeded(state.value.activeAccount, dbDir, bridge)
    }

    // -------------------------------------------------------------------------
    // Wallet (NIP-47 / NWC)
    // -------------------------------------------------------------------------

    /** Connect a NIP-47 wallet via NWC URI. [actionJson] = {"Connect":{"uri":"nostr+walletconnect://..."}} */
    fun dispatchWalletConnect(actionJson: String) {
        val response = bridge.dispatchAction("nmp.wallet.connect", actionJson)
        Log.d(TAG, "wallet connect response: $response")
    }

    /** Disconnect the current NIP-47 wallet. */
    fun dispatchWalletDisconnect() {
        val response = bridge.dispatchAction("nmp.wallet.disconnect", "\"Disconnect\"")
        Log.d(TAG, "wallet disconnect response: $response")
    }

    private fun dispatchTypedIntent(intent: ChirpActionIntent): DispatchResult? {
        val intentJson = chirpActionJson.encodeToString(intent)
        val specResponse = bridge.buildActionSpec(intentJson)
        val spec = try {
            chirpActionJson.decodeFromString<ChirpActionSpec>(specResponse)
        } catch (e: Exception) {
            Log.d(TAG, "buildActionSpec parse error: $specResponse", e)
            return null
        }
        if (spec.error != null) {
            Log.d(TAG, "buildActionSpec rejected ${intent.type}: ${spec.error}")
            return null
        }
        if (spec.namespace.isBlank() || spec.bodyJson.isBlank()) {
            Log.d(TAG, "buildActionSpec missing dispatch fields: $specResponse")
            return null
        }
        val response = bridge.dispatchAction(spec.namespace, spec.bodyJson)
        Log.d(TAG, "dispatchTypedIntent(${intent.type}) response: $response")
        return response
    }

    /**
     * Decode one FlatBuffers update frame (single pass: SnapshotEnvelope + typed
     * nmp.feed.home sidecar). Returns null on parse error (fail-closed, D1).
     * Panic frames latch [kernelIsDead] (D7).
     */
    private fun decodeUpdate(bytes: ByteArray): KernelUpdate? {
        return when (val frame = KernelUpdateFrameDecoder.decode(bytes)) {
            null -> null
            is KernelDecodedUpdateFrame.Panic -> {
                Log.wtf(TAG, "NMP_ACTOR_PANIC: ${frame.message}")
                _kernelIsDead.value = true
                null
            }
            is KernelDecodedUpdateFrame.Snapshot -> {
                // Prefer typed nmp.feed.home sidecar (ADR-0038 V-85 / ADR-0037 C4).
                val typed: ChirpOpFeedSnapshot? =
                    TypedHomeFeedDecoder.decode(frame.typedProjections)
                if (typed != null) frame.update.copy(modularTimeline = typed) else frame.update
            }
        }
    }

    override fun onCleared() {
        val signerJob = signerReaderJob
        signerReaderJob = null
        started = false
        bridge.stop()
        // `closeUpdates` quiesces the kernel update callback and drops the push
        // listener (issue #614) — no reader coroutine to join anymore.
        bridge.closeUpdates()
        runBlocking {
            signerJob?.cancelAndJoin()
        }
        bridge.free()
        super.onCleared()
    }
}
