package org.nmp.android

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
 * Observable mirror of the kernel snapshot — the Android peer of iOS
 * `KernelModel`. The Rust actor pushes FlatBuffers `UpdateFrame` bytes
 * (file_identifier "NMPU"); a reader coroutine decodes them via
 * [KernelUpdateFrameDecoder] and republishes via [StateFlow].
 *
 * Pure mirror: the only guard is `rev` monotonicity (identical to the Swift
 * `guard update.rev > rev` in `apply`). No Kotlin-side business logic or
 * derived state (D5/D8); decode fails closed (D1).
 *
 * Each [ByteArray] from `nextUpdate()` carries a [SnapshotEnvelope] (the
 * ADR-0044 Tier-3 typed envelope decoded directly from `SnapshotFrame` fields)
 * AND typed sidecars in `SnapshotFrame.typed_projections`, including the
 * `nmp.feed.home` FlatBuffers projection (file_identifier "NFTS"). Both are
 * extracted in a single pass through [KernelUpdateFrameDecoder.decode] — no
 * second FFI call needed. `payload:Value` was removed from the wire in PR #1082
 * (PR-B); the typed-sidecar path is now authoritative on all platforms.
 */
class KernelModel : ViewModel() {

    private val bridge = KernelBridge()

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

    /**
     * ADR-0048 D6 (generalises V-14 / #963): unified remote-signer health.
     * Null while no remote-signer session is active (local-key accounts — the
     * steady state). Covers BOTH NIP-46 bunker and NIP-55 (Amber) sessions.
     * Collected from `projections.signerState`; decoded via the JSON fallback
     * path (Android does not use a typed FlatBuffers sidecar for this
     * projection). `isReady` = green, `isAwaitingApproval`/`isReconnecting` =
     * amber (wait), `isUnavailable`/`isFailed` = red (re-auth). Drives
     * `SignerStateRow` in the sign-in screen.
     */
    val signerState: StateFlow<SignerState?> =
        state.map { it.projections?.signerState }
            .stateIn(viewModelScope, SharingStarted.Eagerly, null)

    private var started = false
    private var readerJob: Job? = null
    private var signerReaderJob: Job? = null
    private var lastLoadMoreCursor: TimelineWindowCursor? = null

    /**
     * ADR-0048 Stage 2 — the host adapter that executes NIP-55 capability
     * requests (`ExternalSignerCapabilityBridge.handleJson`). Registered by
     * the activity (it owns the Activity Result launcher); the signer reader
     * loop hands each drained request JSON to this handler on Main (Intent
     * launches require the main thread). Null = no activity registered yet;
     * the request is dropped and degrades to a Rust-side timeout (D6).
     */
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

    fun start(storagePath: String? = null) {
        if (started) return
        started = true
        if (!storagePath.isNullOrBlank()) {
            bridge.setStoragePath(storagePath)
        }
        bridge.start(visibleLimit = 80, emitHz = 4)
        readerJob = viewModelScope.launch(Dispatchers.IO) {
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
                _state.value = decoded
                _snapshotCount.value += 1
                _lastSnapshotAtMs.value = System.currentTimeMillis()
            }
        }
        // ADR-0048 Stage 2 — drain Rust-built NIP-55 capability requests and
        // hand them to the activity-registered bridge on Main (Intent
        // launches require the main thread). Same blocking-timed-drain shape
        // as the update reader above.
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
                    // No activity registered — drop; the Rust op times out
                    // and surfaces a D6 toast. Never queue stale Intents.
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
     * Encode a hex pubkey as a NIP-19 display identifier via the kernel's
     * cached kind:10002 relay hints (`nprofile1…`) or a bare `npub1…` when
     * none are available. ADR-0032 / V-115: replaces the deprecated
     * `ProfileCard.npub` projection field.
     *
     * Returns null when the bridge is uninitialised or the pubkey is invalid;
     * callers should fall back to short-hex rendering in that case.
     */
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

    /**
     * Extend the Rust-owned home-feed window after the rendered tail becomes
     * visible. Android treats [after] as an opaque edge marker; Rust owns page
     * size, cap, cursor interpretation, and the next snapshot.
     */
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

    /**
     * ADR-0048 Stage 2 — begin a NIP-55 sign-in with the given detected
     * signer. Kotlin reports user intent only; Rust builds the
     * `get_public_key` + permission-batch request (D7) and routes it back
     * through the registered capability bridge.
     */
    fun signInWithAmber(signer: NostrSignerInfo) {
        bridge.signInNip55(signer.contentAuthority)
    }

    /**
     * ADR-0048 Stage 2 — route a raw `ExternalSignerResponse` JSON from the
     * capability bridge back to the Rust NIP-55 driver (D7: verbatim).
     */
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
        // Mirror desktop bridge: openTimeline after account creation so the
        // kernel starts fetching notes for the new account immediately.
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
    // Marmot (MLS-over-Nostr encrypted groups)
    // -------------------------------------------------------------------------
    //
    // Android peer of iOS `MarmotStore` (Bridge/MarmotBridge.swift). State is
    // read reactively from the `nmp.marmot.snapshot` / `nmp.marmot.messages`
    // push projections on `state` (V-107 / ADR-0039) — never polled. Write ops
    // route through the generic `dispatch_action("nmp.marmot", …)` seam; the
    // refreshed snapshot arrives on the next kernel tick.

    /** Account this `KernelModel` last registered a Marmot identity for. */
    private var marmotRegisteredAccount: String? = null

    /**
     * Register a Marmot MLS identity against the active local account, idempotent
     * per account. [dbDir] is the host app-support directory (e.g.
     * `context.filesDir.path`). No-op when there is no active account yet, or
     * when already registered for the current account. Mirrors the iOS shell
     * calling `registerActiveMarmotIfAvailable()` once an account is live.
     */
    fun registerMarmotIfNeeded(dbDir: String) {
        val account = state.value.activeAccount
        if (account.isEmpty() || account == marmotRegisteredAccount) return
        if (bridge.marmotRegisterActive(dbDir)) {
            marmotRegisteredAccount = account
        }
    }

    /**
     * Create a new MLS group. [inviteeText] is the raw text the user typed;
     * Rust tokenises (whitespace / comma / semicolon / newline) and validates
     * each entry — no parsing in Kotlin. Fire-and-forget: the new group appears
     * on the next snapshot tick.
     */
    fun createGroup(name: String, description: String, inviteeText: String) =
        bridge.dispatchAction(
            "nmp.marmot",
            """{"op":"create_group","name":"${escapeJson(name)}","description":"${escapeJson(description)}","invitee_text":"${escapeJson(inviteeText)}","signed_key_package_events_json":[]}""",
        )

    /** Send an application message in an existing MLS group. */
    fun sendGroupMessage(groupIdHex: String, text: String) =
        bridge.dispatchAction(
            "nmp.marmot",
            """{"op":"send","group_id_hex":"$groupIdHex","text":"${escapeJson(text)}"}""",
        )

    /** Publish (or rotate) the local MLS key package. */
    fun publishKeyPackage() =
        bridge.dispatchAction("nmp.marmot", """{"op":"publish_key_package"}""")

    /** Accept a pending MLS group invite (kind:444 Welcome). */
    fun acceptWelcome(welcomeIdHex: String) =
        bridge.dispatchAction("nmp.marmot", """{"op":"accept_welcome","welcome_id_hex":"$welcomeIdHex"}""")

    /** Decline a pending MLS group invite. */
    fun declineWelcome(welcomeIdHex: String) =
        bridge.dispatchAction("nmp.marmot", """{"op":"decline_welcome","welcome_id_hex":"$welcomeIdHex"}""")

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
     * Decode one FlatBuffers update frame.
     *
     * Decodes the [SnapshotEnvelope] (ADR-0044 Tier-3 fields directly on
     * `SnapshotFrame`) and the typed `nmp.feed.home` timeline projection
     * (from `SnapshotFrame.typed_projections`, file_identifier "NFTS") in a
     * single pass.  `payload:Value` is no longer emitted (PR #1082 — PR-B);
     * the typed-sidecar path is authoritative.  Returns `null` (drop the
     * frame) on any parse error; logs enough context to diagnose the failure
     * without flooding logcat (PD-025 finding 4 — no silent swallow).
     *
     * Panic frames are logged at ASSERT level — they indicate actor death (D7)
     * and also latch [kernelIsDead], matching the iOS fatal-kernel state.
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
        val job = readerJob
        readerJob = null
        val signerJob = signerReaderJob
        signerReaderJob = null
        started = false
        bridge.stop()
        bridge.closeUpdates()
        runBlocking {
            job?.cancelAndJoin()
            signerJob?.cancelAndJoin()
        }
        bridge.free()
        super.onCleared()
    }
}
