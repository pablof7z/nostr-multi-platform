package org.nmp.android

/**
 * Push callback for kernel update frames (issue #614 — D8 no-polling).
 *
 * Rust invokes [onUpdate] from the kernel's update-listener thread (a native
 * background thread), NOT the Android main thread. Implementations must marshal
 * to the main thread themselves if they touch UI state directly. [frame] is one
 * FlatBuffers `UpdateFrame` (file_identifier "NMPU").
 */
fun interface KernelUpdateListener {
    fun onUpdate(frame: ByteArray)
}

/**
 * Push callback for NIP-55 external-signer requests (issue #1284 — D8
 * no-polling; replaces the former 250 ms `nextSignerRequest` drain loop).
 *
 * Rust invokes [onSignerRequest] from whichever thread dispatches the
 * `external_signer` capability (a native background thread), NOT the Android
 * main thread. The NIP-55 launch Intent must run on the main thread, so
 * implementations marshal there themselves. [requestJson] is one
 * `ExternalSignerRequest` JSON for `ExternalSignerCapabilityBridge.handleJson`.
 */
fun interface KernelSignerRequestListener {
    fun onSignerRequest(requestJson: String)
}

/**
 * JNI/UniFFI wrapper around `libnmp_android_ffi.so` (M14-0 / issue #2129).
 *
 * App-loop lifecycle (create/start/stop/close/free) and update/dispatch lanes
 * are served by the UniFFI [AppHandle] object (proc-macro bindings, JNA runtime).
 * Residual JNI lanes (signer, capability, marmot, identity, feeds, claims) remain
 * on the existing `#[no_mangle] Java_*` JNI path and are reached via [rawHandle].
 *
 * Doctrine: no business logic or cached state (D5/D8). Runtime outcomes arrive
 * in the next update frame. Init-only config calls may return `NmpConfigStatus`
 * codes so ordering mistakes fail loudly without Android policy.
 */
class KernelBridge {
    /** UniFFI [AppHandle] for the app-loop lane (M14-0). Null only after [free]. */
    @Volatile
    private var appHandle: org.nmp.android.uniffi.AppHandle? = null

    /**
     * Legacy JNI session registry id, obtained from [org.nmp.android.uniffi.AppHandle.legacyJniSessionId]
     * after construction. Used by residual JNI lanes (signer, capability,
     * marmot, identity, feeds, claims) until they are individually migrated.
     * Zero when [appHandle] is null (post-[free] or inert-init).
     */
    @Volatile
    private var handle: Long = 0

    private var homeFeedHandle: String? = null
    private val threadFeedHandles = mutableMapOf<String, String>()
    private val authorFeedHandles = mutableMapOf<String, String>()

    init {
        System.loadLibrary("nmp_android_ffi")
        // UniFFI app-loop lane (M14-0): AppHandle() calls nmp_app_new / init /
        // register internally. D6: init failure yields an inert AppHandle whose
        // dispatch methods return DispatchAck.error and whose lifecycle methods
        // are no-ops. legacyJniSessionId() returns 0 for an inert handle.
        appHandle = org.nmp.android.uniffi.AppHandle()
        handle = appHandle!!.legacyJniSessionId()
    }

    /** Configure the Rust LMDB storage directory before [start]. */
    fun setStoragePath(path: String) {
        if (handle != 0L) {
            val status = nativeSetStoragePath(handle, path)
            check(status == 0) { "nativeSetStoragePath failed with NmpConfigStatus=$status" }
        }
    }

    fun start(visibleLimit: Int = 80, emitHz: Int = 4) {
        if (handle != 0L) appHandle?.start(visibleLimit.toUInt(), emitHz.toUInt())
    }

    fun stop() {
        if (handle != 0L) appHandle?.stop()
    }

    /**
     * Quiesce the kernel update callback and deregister the [KernelUpdateListener].
     *
     * Blocks until any in-flight `on_update` invocation completes (lockfree
     * quiescence — [org.nmp.android.uniffi.AppHandle.clearUpdateSink]).
     * Call before [free]. D6: null/dead handle is a no-op.
     */
    fun closeUpdates() {
        if (handle != 0L) appHandle?.clearUpdateSink()
    }

    fun lifecycleForeground() {
        if (handle != 0L) nativeLifecycleForeground(handle)
    }

    fun lifecycleBackground() {
        if (handle != 0L) nativeLifecycleBackground(handle)
    }

    fun isAlive(): Boolean = handle != 0L && nativeIsAlive(handle)

    /** Open the Chirp home feed for primary kind:1 notes. */
    fun openHomeFeed() {
        val current = handle
        if (current != 0L && homeFeedHandle == null) {
            homeFeedHandle = nativeOpenHomeFeed(current)
        }
    }

    fun createLocalAccount(displayName: String = "Android User") {
        if (handle != 0L) nativeCreateLocalAccount(handle, displayName)
    }

    /**
     * Register a push listener for kernel update frames (D8 no-polling).
     *
     * [listener] receives each FlatBuffers `UpdateFrame` (file_identifier
     * "NMPU") on the kernel's update-listener thread — a native background
     * thread, NOT the main thread. Decode with [KernelUpdateFrameDecoder] and
     * marshal to the main thread for UI state.
     *
     * Internally wraps [listener] in [UniFFISinkWrapper] and registers it with
     * [org.nmp.android.uniffi.AppHandle.setUpdateSink] (M14-0). Replacing an
     * existing listener is allowed; pass a new one to swap.
     *
     * Call [clearUpdateListener] (or [closeUpdates]) on teardown before [free].
     * D6: a null/dead handle is a no-op.
     */
    fun setUpdateListener(listener: KernelUpdateListener) {
        if (handle != 0L) appHandle?.setUpdateSink(UniFFISinkWrapper(listener))
    }

    /**
     * Deregister the push listener (M14-0: calls [AppHandle.clearUpdateSink]).
     * Quiescent: waits for any in-flight on_update to complete. D6: no-op.
     */
    fun clearUpdateListener() {
        if (handle != 0L) appHandle?.clearUpdateSink()
    }

    /**
     * Dispatch a pre-built `(namespace, bodyJson)` action through the UniFFI
     * byte doorway (M14-0; replaces `nativeDispatchActionBytes`).
     *
     * Called by sibling extension files: [KernelBridgeOutboxRelay],
     * [KernelBridgeMarmotActions], [KernelBridgeWalletActions].
     * D6: null/dead handle returns [DispatchResult.Failure].
     */
    internal fun dispatchActionJson(namespace: String, bodyJson: String): DispatchResult {
        if (handle == 0L) return DispatchResult.Failure("dispatch returned a null handle")
        val ack = appHandle?.dispatchActionJson(namespace, bodyJson)
            ?: return DispatchResult.Failure("dispatch returned a null handle")
        return DispatchResult.fromAck(ack)
    }

    /**
     * Dispatch a pre-encoded `DispatchEnvelope` FlatBuffers byte buffer through
     * the typed byte doorway (M14-1 / #2145 — generated builder path).
     *
     * Called by [KernelBridgeOutboxRelay] and [KernelBridgeWalletActions] after
     * building bytes via [GeneratedActionBuilders]. App code NEVER spells
     * action namespaces — those live only in the generated builders.
     * D6: null/dead handle returns [DispatchResult.Failure].
     */
    internal fun dispatchBytes(bytes: ByteArray): DispatchResult {
        if (handle == 0L) return DispatchResult.Failure("dispatch returned a null handle")
        val ack = appHandle?.dispatchActionBytes(bytes)
            ?: return DispatchResult.Failure("dispatch returned a null handle")
        return DispatchResult.fromAck(ack)
    }

    /**
     * Acknowledge a terminal `action_stages` entry after the host has reacted.
     * Rust owns the lifecycle ledger; Android forwards only the correlation id.
     */
    fun ackActionStage(correlationId: String) {
        if (handle != 0L) nativeAckActionStage(handle, correlationId)
    }

    /** Ask the Rust-owned feed controller to extend [feedKey]. */
    fun loadOlderFeed(feedKey: String) {
        if (handle != 0L) nativeLoadOlderFeed(handle, feedKey)
    }

    /**
     * Open a thread by note ID. Rust registers `nmp.feed.thread.<noteId>` and
     * admits primary kind:1 notes plus derived repost wrappers for rendering.
     *
     * D6: null handle or invalid note_id is a silent no-op.
     */
    fun openThread(noteId: String) {
        val current = handle
        if (current != 0L && !threadFeedHandles.containsKey(noteId)) {
            nativeOpenThread(current, noteId)?.let { threadFeedHandles[noteId] = it }
        }
    }

    /**
     * Close a thread feed opened with [openThread].
     */
    fun closeThread(noteId: String) {
        val feedHandle = threadFeedHandles.remove(noteId) ?: return
        if (handle != 0L) nativeCloseThread(handle, feedHandle)
    }

    /**
     * Open an author profile by pubkey. Rust registers
     * `nmp.feed.author.<pubkey>` and admits primary kind:1 notes plus derived
     * repost wrappers for rendering. Profile metadata is fetched via refs.profile.
     *
     * D6: null handle or invalid pubkey is a silent no-op.
     */
    fun openAuthor(pubkey: String) {
        val current = handle
        if (current != 0L && !authorFeedHandles.containsKey(pubkey)) {
            nativeOpenAuthor(current, pubkey)?.let { authorFeedHandles[pubkey] = it }
        }
    }

    /** Close an author feed opened with [openAuthor]. */
    fun closeAuthor(pubkey: String) {
        val feedHandle = authorFeedHandles.remove(pubkey) ?: return
        if (handle != 0L) nativeCloseAuthor(handle, feedHandle)
    }

    /**
     * Seed the relay list from an override JSON string or the Chirp defaults.
     *
     * [relaysJson] is an optional `[["url","role"],…]` JSON array. When null
     * the Chirp reference relays are seeded (normal production path). When
     * non-null the supplied list REPLACES the defaults entirely (E2E test
     * override). All parsing and policy live in Rust (D7 / thin-shell).
     *
     * Must be called AFTER [start] so the kernel is alive to receive the
     * relay entries. D6: null/dead handle or malformed JSON falls back to
     * the Chirp reference relay set.
     */
    fun seedRelays(relaysJson: String? = null) {
        if (handle != 0L) nativeSeedRelays(handle, relaysJson)
    }

    /**
     * Add a relay with the given URL and role ("read", "write", or "both").
     *
     * D6: null handle is a silent no-op.
     */
    fun addRelay(url: String, role: String = "both") {
        if (handle != 0L) nativeAddRelay(handle, url, role)
    }

    /**
     * Remove a relay by URL.
     *
     * D6: null handle is a silent no-op.
     */
    fun removeRelay(url: String) {
        if (handle != 0L) nativeRemoveRelay(handle, url)
    }

    /** Switch the active account to the given pubkey (calls nmp_app_switch_active directly). */
    fun switchAccount(pubkey: String) {
        if (handle != 0L) nativeSwitchAccount(handle, pubkey)
    }

    /** Remove an account by pubkey (calls nmp_app_remove_account directly). */
    fun removeAccount(pubkey: String) {
        if (handle != 0L) nativeRemoveAccount(handle, pubkey)
    }

    /**
     * Register a Marmot (MLS-over-Nostr) projection against the active local
     * account. Direct mirror of iOS
     * `KernelHandle.registerActiveMarmotIfAvailable()`.
     *
     * The secret never crosses the JNI seam — `nmp_marmot_register_active`
     * reads the actor-owned local key from the slot the kernel writes after
     * every identity mutation. [dbDir] is the host app-support directory; the
     * MLS SQLite state lives at `<dbDir>/marmot-mls-state.sqlite`.
     *
     * Returns `true` once a handle is held; `false` when no local signing key
     * is active (signed out, or a bunker/NIP-46 account with no local key).
     * Idempotent — re-registers cleanly (account switch), so callers may invoke
     * it whenever the active account changes.
     *
     * Once registered the kernel pushes `nmp.marmot.snapshot` /
     * `nmp.marmot.messages` projections on every snapshot tick (V-107 /
     * ADR-0039); group write ops route through [dispatchAction] with the
     * `"nmp.marmot"` namespace — there is no per-op native symbol.
     */
    fun marmotRegisterActive(dbDir: String): Boolean =
        if (handle != 0L) nativeMarmotRegisterActive(handle, dbDir) else false

    /**
     * Drop the Marmot observer registration if one exists (sign-out path).
     * Idempotent — safe to call when nothing is registered. [free] performs
     * this implicitly before reclaiming the kernel, so a normal teardown does
     * not need an explicit call.
     */
    fun marmotUnregister() {
        if (handle != 0L) nativeMarmotUnregister(handle)
    }

    /**
     * Encode a hex pubkey as a NIP-19 display identifier (`nprofile1…` when
     * kind:10002 relay hints are cached, else `npub1…`). Wraps the existing
     * `nmp_app_encode_profile` C-ABI symbol — no new NMP C-ABI surface.
     *
     * V-115 / ADR-0032: replaces the removed `ProfileCard.npub` field
     * (fully deleted from profile_card.fbs). Hosts call this to derive the
     * display identifier on their side.
     *
     * D6: returns `null` for a null/dead handle or a malformed pubkey. The
     * caller falls back to its own short-hex rendering in that case.
     */
    fun encodeProfile(pubkey: String): String? =
        if (handle != 0L) nativeEncodeProfile(handle, pubkey) else null

    /**
     * Register the synchronous capability handler for non-`external_signer`
     * namespaces (e.g. Android Keystore keyring). The [handler] object must
     * expose `fun handle(requestJson: String): String`.
     *
     * Must be called BEFORE [identityRestore] so the keyring capability is
     * live when identity-restore reads the persisted secret.
     *
     * D6: null handle is a no-op. D7: the handler executes and reports; Rust
     * owns all policy.
     */
    fun setCapabilityHandler(handler: KeystoreKeyringCapability) {
        if (handle != 0L) nativeSetCapabilityHandler(handle, handler)
    }

    /**
     * Restore a persisted Chirp identity and register Marmot.
     *
     * Calls `nmp_app_chirp_identity_restore` through the JNI seam: the kernel
     * reads the nsec from the keyring capability (via [setCapabilityHandler]),
     * signs in the actor, and registers Marmot against the active key.
     *
     * [dbDir] is the host app-support directory (e.g. `context.filesDir.path`).
     * [testNsec] is null in production; pass a non-null nsec string only in
     * headless UI tests.
     *
     * Returns `true` when a Marmot identity was registered (a local key was
     * found), `false` when no persisted local key exists (first cold-start,
     * bunker / NIP-55 account, or the `marmot` feature is disabled).
     *
     * D6: null/dead handle or any Rust-side failure returns false without
     * panicking across the JNI seam.
     */
    fun identityRestore(dbDir: String, testNsec: String? = null): Boolean =
        handle != 0L && nativeIdentityRestore(handle, dbDir, testNsec)

    /**
     * Expose the raw Android JNI Session pointer (`jlong`) to same-process
     * Android bridge extensions. Returns 0 if the bridge was freed. Callers
     * must not store this value beyond the lifetime of this bridge.
     */
    fun rawHandle(): Long = handle

    fun free() {
        val current = handle
        if (current != 0L) {
            closeOpenFeeds(current)
            // M14-0: shutdown() quiesces the update callback, removes the session
            // from the registry, and frees native resources. Replaces `nativeFree`
            // (deleted in issue #2129). close() (AutoCloseable) then drops the Arc.
            appHandle?.shutdown()
            appHandle?.close()
            appHandle = null
            handle = 0
        }
    }

    private fun closeOpenFeeds(current: Long) {
        homeFeedHandle?.let { nativeCloseFeed(current, it) }
        homeFeedHandle = null
        threadFeedHandles.values.forEach { nativeCloseFeed(current, it) }
        threadFeedHandles.clear()
        authorFeedHandles.values.forEach { nativeCloseFeed(current, it) }
        authorFeedHandles.clear()
    }

    /**
     * Adapts a [KernelUpdateListener] to the UniFFI [org.nmp.android.uniffi.UpdateSink]
     * callback interface (M14-0 / issue #2129).
     *
     * Frames are delivered on the Rust kernel update thread (a native background
     * thread). Implementations of [KernelUpdateListener.onUpdate] must marshal
     * to the Android main thread themselves when touching UI state.
     *
     * Panics inside [KernelUpdateListener.onUpdate] are caught by the Rust
     * `catch_unwind` trampoline (in `uniffi_app_loop.rs::AppHandle::set_update_sink`)
     * and logged/dropped; they do NOT abort the process.
     */
    private inner class UniFFISinkWrapper(
        private val delegate: KernelUpdateListener,
    ) : org.nmp.android.uniffi.UpdateSink {
        override fun onUpdate(frame: ByteArray) {
            delegate.onUpdate(frame)
        }
    }

    // ── Residual JNI external declarations ───────────────────────────────────
    //
    // The following app-loop JNI symbols were DELETED in M14-0 (issue #2129):
    //   nativeNew, nativeStart, nativeStop, nativeClose, nativeFree,
    //   nativeSetUpdateListener, nativeClearUpdateListener,
    //   nativeDispatchIntentBytes, nativeDispatchActionBytes.
    //
    // The remaining declarations below are for residual JNI lanes (signer,
    // capability, marmot, identity, feeds, claims) staged for future migration.
    private external fun nativeSetStoragePath(handle: Long, path: String): Int
    private external fun nativeOpenHomeFeed(handle: Long): String?
    private external fun nativeCreateLocalAccount(handle: Long, displayName: String)
    private external fun nativeLifecycleForeground(handle: Long)
    private external fun nativeLifecycleBackground(handle: Long)
    private external fun nativeIsAlive(handle: Long): Boolean
    // Ref resolution — `internal` so the cohesive wrappers live in the sibling
    // KernelBridgeRefs.kt without inflating this file past the LOC ceiling. Event URI
    // JNI functions below are app-local adapters over `resolve_ref` / `release_ref`.
    internal external fun nativeClaimEvent(handle: Long, uri: String, consumerId: String)
    internal external fun nativeReleaseEvent(handle: Long, uri: String, consumerId: String)
    internal external fun nativeResolveRef(
        handle: Long,
        namespace: Int,
        key: String,
        consumerId: String,
        shape: Int,
        liveness: Int,
    )
    internal external fun nativeReleaseRef(handle: Long, namespace: Int, key: String, consumerId: String)
    private external fun nativeAckActionStage(handle: Long, correlationId: String)
    // Outbox control-plane (parity GAP 4). `internal` so the cohesive
    // [retryPublish]/[cancelPublish] wrappers can live in the sibling
    // KernelBridgeOutboxRelay.kt without inflating this file past the LOC ceiling.
    internal external fun nativeRetryPublish(handle: Long, correlationId: String)
    internal external fun nativeCancelPublish(handle: Long, correlationId: String)
    private external fun nativeLoadOlderFeed(handle: Long, feedKey: String)
    private external fun nativeOpenThread(handle: Long, noteId: String): String?
    private external fun nativeCloseThread(handle: Long, feedHandle: String)
    private external fun nativeOpenAuthor(handle: Long, pubkey: String): String?
    private external fun nativeCloseAuthor(handle: Long, feedHandle: String)
    private external fun nativeCloseFeed(handle: Long, feedHandle: String)
    private external fun nativeSeedRelays(handle: Long, relaysJson: String?)
    private external fun nativeAddRelay(handle: Long, url: String, role: String)
    private external fun nativeRemoveRelay(handle: Long, url: String)
    // Signer JNI symbols — `internal` so KernelBridgeSignerActions.kt extension
    // functions can reach them without inflating this file past the LOC ceiling.
    internal external fun nativeSignInNsec(handle: Long, secret: String)
    internal external fun nativeSignInBunker(handle: Long, uri: String)
    internal external fun nativeCancelBunkerHandshake(handle: Long)
    internal external fun nativeSignInNip55(handle: Long, signerPackage: String?)
    internal external fun nativeSetSignerRequestListener(handle: Long, listener: KernelSignerRequestListener)
    internal external fun nativeClearSignerRequestListener(handle: Long)
    internal external fun nativeDeliverSignerResponse(handle: Long, responseJson: String)
    internal external fun nativeNostrConnectUri(handle: Long, callbackScheme: String?): String?
    private external fun nativeSwitchAccount(handle: Long, pubkey: String)
    private external fun nativeRemoveAccount(handle: Long, pubkey: String)
    private external fun nativeMarmotRegisterActive(handle: Long, dbDir: String): Boolean
    private external fun nativeMarmotUnregister(handle: Long)
    private external fun nativeEncodeProfile(handle: Long, pubkey: String): String?
    private external fun nativeSetCapabilityHandler(handle: Long, handler: Any)
    private external fun nativeIdentityRestore(handle: Long, dbDir: String, testNsec: String?): Boolean
}
