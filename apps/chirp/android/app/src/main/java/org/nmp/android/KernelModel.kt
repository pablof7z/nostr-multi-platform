package org.nmp.android

import android.content.Context
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
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

    // `internal` so sibling extension files (KernelModelAccounts, KernelModelActions)
    // in the same package can call bridge methods without re-exposing them as public.
    internal val bridge = KernelBridge()

    // ADR-0055 R3-S4: NMP-owned rev-aware projection cache. One instance per
    // kernel session. Reset on session teardown (onCleared). Fed each FlatBuffers
    // frame in decodeUpdate before the TypedXDecoder family runs.
    private val projectionCache = ProjectionMergeCache()

    // ADR-0063 Lane G (#1671): NMP-owned per-key (row-keyed) reference cache for
    // the keyed reference projections (`refs.profile` / `refs.event`). The Android
    // twin of iOS `KernelHandle.keyedRefCache`. One instance per kernel session;
    // fed the RAW `nmp.refs.RefRowDeltaBatch` (NRRD) sidecar envelopes each frame
    // in decodeUpdate (NOT through ProjectionMergeCache, which is keyed per WHOLE
    // projection, not per row). Profiles are read per-key via [profileCard].
    private val keyedRefCache = KeyedRefCache()

    // ADR-0063 Lane G: per-key row-change broadcast. The cache notifies one
    // [KeyedRowChange] per committed/cleared key; we re-emit it on this flow so a
    // single avatar/name leaf observes ONLY its own pubkey (per-key reactivity,
    // not a whole-map StateFlow broadcast — the iOS `rowChanged` publisher twin).
    private val _profileRowChanged =
        kotlinx.coroutines.flow.MutableSharedFlow<KeyedRowChange>(
            extraBufferCapacity = 256,
            onBufferOverflow = kotlinx.coroutines.channels.BufferOverflow.DROP_OLDEST,
        )
    /** Per-key profile row-change events — one per committed/cleared `refs.profile` key. */
    val profileRowChanged: SharedFlow<KeyedRowChange> =
        _profileRowChanged.asSharedFlow()

    init {
        // Bridge the cache's synchronous row-change listener (invoked on the
        // native update-listener thread) onto the shared flow consumers observe.
        keyedRefCache.addRowChangeListener { change -> _profileRowChanged.tryEmit(change) }
    }

    /**
     * Marmot (MLS-over-Nostr encrypted groups) write operations. Mirrors the iOS
     * `model.marmot` surface; all UI call sites use `model.marmot.<op>()`.
     * Extracted into [MarmotActions] to keep this file under the 500-LOC ceiling.
     */
    val marmot = MarmotActions(dispatchBytes = { bytes -> bridge.dispatchMarmotBytes(bytes) })

    /**
     * Social write operations (NIP-25/57/18/02/17). Extracted into [SocialActions]
     * to keep this file under the 500-LOC ceiling. The public `zapNote`/`react`/
     * `repost`/`follow`/`unfollow`/`sendDm` methods below delegate to it, so the
     * call-site surface (`model.zapNote(…)` etc.) is unchanged.
     */
    // `internal` so KernelModelActions.kt (same package) can delegate social write
    // ops without re-exposing the SocialActions object as public.
    internal val social = SocialActions(
        dispatchBytes = { bytes -> bridge.dispatchBytes(bytes) },
    )

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
     * Cold-start the kernel: install the Keystore keyring capability, start the
     * actor (which runs the Rust-side identity-restore read chain), wire the
     * push listeners, and register Marmot against the restored key.
     *
     * The keyring capability + identity-restore are UNCONDITIONAL (production
     * and debug). This is the fix for the v1 regression where they only ran on a
     * `BuildConfig.DEBUG`-gated test path: the kernel persists the active secret
     * on sign-in and restores it on launch exclusively through this capability,
     * so without it a signed-in user was logged out on every restart.
     *
     * [testNsec] is null in production; pass a non-null nsec only in headless UI
     * tests (it signs that secret in instead of reading the persisted one).
     * [testRelays] is null in production; when non-null it must be a JSON array
     * of `["url","role"]` pairs that REPLACES the Chirp reference relays — used
     * by E2E harnesses to point the kernel at a local relay (e.g. `nak serve`).
     * Both ride on top of the SAME launch path; they never select an alternate
     * orchestration. Kotlin ferries them verbatim; all parsing and policy live
     * in Rust (D7 / thin-shell principle).
     */
    fun start(
        context: Context,
        storagePath: String? = null,
        testNsec: String? = null,
        testRelays: String? = null,
    ) {
        if (started) return
        started = true
        planKernelLaunch(
            seam = BridgeLaunchSeam(
                bridge = bridge,
                // Installed unconditionally (production AND debug), mirroring iOS,
                // which registers its keychain capability in `KernelModel.init`.
                // The JNI bridge holds the GlobalRef once installed, so no Kotlin
                // field needs to retain it.
                capability = KeystoreKeyringCapability(context.applicationContext),
                applyFrame = { bytes -> applyFrame(bytes) },
                onSignerRequest = { requestJson -> dispatchSignerRequestToMain(requestJson) },
            ),
            storagePath = storagePath,
            dbDir = storagePath ?: context.filesDir.path,
            testNsec = testNsec,
            testRelays = testRelays,
        )
    }

    /**
     * Decode one pushed frame and republish it (mirror-only, rev-monotonic).
     * Invoked on the kernel's update-listener thread (issue #614); single-writer
     * kernel keeps the rev read/write correct and the assignment thread-safe.
     */
    private fun applyFrame(bytes: ByteArray) {
        val decoded = decodeUpdate(bytes) ?: return
        if (decoded.rev <= _state.value.rev) return  // mirror only
        _state.value = decoded
        _snapshotCount.value += 1
        _lastSnapshotAtMs.value = System.currentTimeMillis()
    }

    /**
     * Hop a pushed NIP-55 request (ADR-0048 Stage 2 / #1284) onto the main
     * thread before handing it to the activity-owned handler — the NIP-55 launch
     * Intent requires the main thread. Rust invokes the listener on its native
     * capability-dispatch thread.
     */
    private fun dispatchSignerRequestToMain(requestJson: String) {
        externalSignerHandler?.let { handler ->
            viewModelScope.launch(Dispatchers.Main) { handler(requestJson) }
        } ?: Log.w(TAG, "NIP-55 request dropped: no capability bridge registered")
    }

    /** Open the Chirp home feed for primary kind:1 notes. Reposts are derived in Rust. */
    fun openHomeFeed() {
        bridge.openHomeFeed()
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
     * ADR-0063 Lane G (#1671) — resolve a profile reference via the unified
     * `resolve_ref` seam (Profile namespace), the Android twin of iOS
     * `resolveProfile`. Replaces the legacy `claim_profile` path: the kernel
     * resolves the kind:0 and ships it in the next `refs.profile` (NRRD) sidecar,
     * read per-key via [profileCard].
     *
     * [shape] — [RefShape.ProfileRef] for feed/list/avatar (the small subset),
     *   [RefShape.ProfileCard] for the open profile screen (the full card).
     * [liveness] — [RefLiveness.CacheOk] for background/feed rows,
     *   [RefLiveness.Live] for the open screen (keeps a tailing sub).
     */
    fun resolveProfile(
        pubkey: String,
        consumerId: String,
        shape: RefShape = RefShape.ProfileRef,
        liveness: RefLiveness = RefLiveness.CacheOk,
    ) {
        bridge.resolveRef(RefNamespace.Profile, pubkey, consumerId, shape, liveness)
    }

    /** Inverse of [resolveProfile]; safe to call even if no matching ref is live. */
    fun releaseProfile(pubkey: String, consumerId: String) {
        bridge.releaseRef(RefNamespace.Profile, pubkey, consumerId)
    }

    /**
     * ADR-0063 Lane G (#1671) — the per-key TYPED profile read backed by the
     * `refs.profile` keyed-ref cache (the source of truth, D4 — no app-side
     * profile cache). The iOS `profileCard(forPubkey:)` twin. Returns null until
     * the kernel resolves the kind:0 for [pubkey]; a leaf re-reads this when
     * [profileRowChanged] fires for its key so exactly one avatar re-renders.
     */
    fun profileCard(pubkey: String): org.nmp.android.model.ProfileCard? =
        keyedRefCache.profile(pubkey)

    /**
     * ADR-0063 Lane G/H (#1671) — the per-key TYPED event read backed by the
     * `refs.event` keyed-ref cache. This is the live event-reference source of
     * truth; legacy embed sidecars are derived from the same rows. Returns null
     * until the kernel resolves [primaryId].
     */
    fun refEvent(primaryId: String): ClaimedEventDto? = keyedRefCache.event(primaryId)

    /**
     * App-local URI adapter for demand-driven embedded-event fetches (#984):
     * the UI is rendering an
     * out-of-feed `EventRef` [uri] under [consumerId]; the kernel resolves it
     * and ships the typed row in `refs.event`; Chirp also receives the derived
     * `refs.event.envelopes` sidecar (`projections.refEventEnvelopes`).
     * Compose DisposableEffect → claim.
     */
    fun claimEvent(uri: String, consumerId: String) {
        bridge.claimEvent(uri, consumerId)
    }

    /** Inverse of [claimEvent]; safe to call even if no matching claim is live. */
    fun releaseEvent(uri: String, consumerId: String) {
        bridge.releaseEvent(uri, consumerId)
    }

    /** NIP-19 display identifier (nprofile1… or npub1…). ADR-0032 / V-115. */
    fun encodeProfile(pubkey: String): String? = bridge.encodeProfile(pubkey)

    /**
     * Publish a new note. Delegates to [SocialActions]; Rust builds the
     * `nmp.publish` namespace and `PublishRaw` body, including reply tags.
     * Returns the correlation_id if accepted, or null on error.
     */
    fun publishNote(content: String, replyToId: String? = null): String? =
        social.publishNote(content, replyToId)

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

    // Action dispatch, wallet, and outbox control-plane are extension functions in
    // KernelModelActions.kt (same package). See also KernelModelAccounts.kt.

    // Account management, relay management, and social/wallet write ops are
    // extension functions in KernelModelAccounts.kt and KernelModelActions.kt
    // (same package, no import required). Extracted to keep this file under the
    // 500-LOC ceiling (AGENTS.md File Size). Public API surface is unchanged.

    // -------------------------------------------------------------------------
    // Marmot registration trampoline — write ops live in [marmot: MarmotActions]
    // -------------------------------------------------------------------------

    /** Idempotent per-account Marmot MLS registration. [dbDir] = context.filesDir.path. */
    fun registerMarmotIfNeeded(dbDir: String) {
        marmot.registerIfNeeded(state.value.activeAccount, dbDir, bridge)
    }

    /**
     * Decode one FlatBuffers update frame (single pass: SnapshotEnvelope + typed
     * nmp.feed.home sidecar). Returns null on parse error (fail-closed, D1).
     * Panic frames latch [kernelIsDead] (D7).
     *
     * ADR-0055 R3-S4: runs ProjectionMergeCache.merge() BEFORE the TypedXDecoder
     * family. The cache implements the D3-3 algorithm: omitted keys retain their
     * prior cached value, Cleared keys are removed, Changed keys are rev-guarded
     * and decode-before-commit guarded. The merged envelope set (not the raw wire
     * set) is fed to decodeProjections() in KernelUpdateFrameDecoder.
     */
    @OptIn(ExperimentalUnsignedTypes::class)
    private fun decodeUpdate(bytes: ByteArray): KernelUpdate? {
        return when (val frame = KernelUpdateFrameDecoder.decode(bytes)) {
            null -> null
            is KernelDecodedUpdateFrame.Panic -> {
                Log.wtf(TAG, "NMP_ACTOR_PANIC: ${frame.message}")
                _kernelIsDead.value = true
                null
            }
            is KernelDecodedUpdateFrame.Snapshot -> {
                // ADR-0055 R3-S4: merge incoming envelopes into the cache.
                // The cache resets internally on session/epoch change (D4).
                val mergeResult = projectionCache.merge(
                    envelopes = frame.typedProjections,
                    sessionId = frame.sessionId,
                    snapshotEpoch = frame.snapshotEpoch,
                )
                if (mergeResult.needsResync) {
                    Log.e(TAG, "projection cache needsResync — decode-before-commit failed for " +
                        "one or more keys; changedKeys=${mergeResult.changedKeys}")
                }
                if (mergeResult.changedKeys.isNotEmpty()) {
                    Log.d(TAG, "projection cache merge: changedKeys=${mergeResult.changedKeys}")
                }
                // ADR-0063 Lane G (#1671): feed the keyed reference projections
                // (`refs.profile` / `refs.event`) into the per-key KeyedRefCache.
                // They are NOT routed through ProjectionMergeCache (which is keyed
                // per WHOLE projection, not per row); they carry an
                // `nmp.refs.RefRowDeltaBatch` (NRRD) payload the per-key cache
                // merges row-by-row. Use the RAW pre-merge envelopes (the verbatim
                // wire payload, untouched by the projection-cache pass) so each
                // committed row fires `profileRowChanged` for exactly its key.
                for (envelope in frame.typedProjections) {
                    if (!envelope.key.startsWith("refs.")) continue
                    keyedRefCache.merge(
                        projectionKey = envelope.key,
                        payload = envelope.payload,
                        sessionId = frame.sessionId,
                        snapshotEpoch = frame.snapshotEpoch,
                    )
                }
                // Re-decode using the MERGED envelope set (cached values for omitted keys
                // are reinstated; Cleared keys are absent; Changed keys carry fresh bytes).
                // Replace the projections slot on the already-decoded KernelUpdate with the
                // merged version — all other fields (rev, running, metrics, relayStatuses,
                // lastErrorToast) were decoded from Tier-3 SnapshotFrame fields and are
                // correct already.
                val mergedProjections = KernelUpdateFrameDecoder.decodeProjections(
                    mergeResult.mergedEnvelopes,
                )
                val mergedUpdate = frame.update.copy(projections = mergedProjections)
                // Prefer typed nmp.feed.home sidecar (ADR-0038 V-85 / ADR-0037 C4).
                val typed: ChirpOpFeedSnapshot? =
                    TypedHomeFeedDecoder.decode(mergeResult.mergedEnvelopes)
                if (typed != null) mergedUpdate.copy(modularTimeline = typed) else mergedUpdate
            }
        }
    }

    override fun onCleared() {
        started = false
        bridge.stop()
        // `closeUpdates` quiesces the kernel update callback and drops both push
        // listeners — the update listener (issue #614) and the NIP-55
        // signer-request listener (issue #1284). No reader coroutine to join.
        bridge.closeUpdates()
        bridge.free()
        // ADR-0055 R3-S4: reset the projection cache so the next session
        // starts from a clean baseline (D4 mandatory reset on session end).
        projectionCache.reset()
        // ADR-0063 Lane G: reset the keyed-ref cache too (D4 — the next
        // refs.profile / refs.event frame is a full baseline after teardown).
        keyedRefCache.reset()
        super.onCleared()
    }
}
