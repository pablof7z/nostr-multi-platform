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
 * Thin JNI wrapper around `libnmp_android_ffi.so`.
 *
 * Doctrine: no business logic or cached state (D5/D8). Runtime outcomes arrive
 * in the next update frame. Init-only config calls may return `NmpConfigStatus`
 * codes so ordering mistakes fail loudly without Android policy.
 */
class KernelBridge {
    @Volatile
    private var handle: Long = 0

    init {
        System.loadLibrary("nmp_android_ffi")
        handle = nativeNew()
    }

    /** Configure the Rust LMDB storage directory before [start]. */
    fun setStoragePath(path: String) {
        if (handle != 0L) {
            val status = nativeSetStoragePath(handle, path)
            check(status == 0) { "nativeSetStoragePath failed with NmpConfigStatus=$status" }
        }
    }

    fun start(visibleLimit: Int = 80, emitHz: Int = 4) {
        if (handle != 0L) nativeStart(handle, visibleLimit, emitHz)
    }

    fun stop() {
        if (handle != 0L) nativeStop(handle)
    }

    /**
     * Close the Rust update callback without freeing the session id.
     *
     * Quiesces the kernel update callback (the Rust gate blocks until any
     * in-flight `on_update` returns) and drops the registered
     * [KernelUpdateListener]. Lifecycle invariant: call [clearUpdateListener]
     * (or this, which also clears it) before [free].
     */
    fun closeUpdates() {
        val current = handle
        if (current != 0L) nativeClose(current)
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
        if (handle != 0L) nativeOpenHomeFeed(handle)
    }

    fun createLocalAccount(displayName: String = "Android User") {
        if (handle != 0L) nativeCreateLocalAccount(handle, displayName)
    }

    /**
     * Register a push listener for kernel update frames (issue #614 — D8
     * no-polling; replaces the former blocking `nextUpdate` drain).
     *
     * [listener] receives each FlatBuffers `UpdateFrame` (file_identifier
     * "NMPU") on the kernel's update-listener thread — a native background
     * thread, NOT the main thread. Decode with [KernelUpdateFrameDecoder] and
     * marshal to the main thread for UI state. Replacing an existing listener
     * is allowed; pass a new one to swap.
     *
     * Call [clearUpdateListener] (or [closeUpdates]) on teardown before [free].
     * D6: a null/dead handle is a no-op.
     */
    fun setUpdateListener(listener: KernelUpdateListener) {
        if (handle != 0L) nativeSetUpdateListener(handle, listener)
    }

    /**
     * Deregister the push listener set by [setUpdateListener]. Safe to call
     * when none is registered. D6: a null/dead handle is a no-op.
     */
    fun clearUpdateListener() {
        if (handle != 0L) nativeClearUpdateListener(handle)
    }

    /**
     * Dispatch a named action through the action registry.
     *
     * Returns the parsed Rust dispatch envelope:
     * * `Accepted(correlation_id)` — the action was accepted and assigned
     *   a correlation id.
     * * `Failure(message)` — the action was rejected, the handle is null, or
     *   Rust returned a malformed envelope.
     */
    fun dispatchAction(namespace: String, actionJson: String): DispatchResult =
        if (handle != 0L) {
            DispatchResult.parse(nativeDispatchAction(handle, namespace, actionJson))
        } else {
            DispatchResult.Failure("dispatch returned a null handle")
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
     * Build a Chirp action dispatch spec from typed user intent.
     *
     * Returns `{"namespace":"...","body_json":"..."}` on success or
     * `{"error":"..."}` on malformed intent. Kotlin passes user input only;
     * Rust owns the action envelope and namespace.
     */
    fun buildActionSpec(intentJson: String): String = nativeBuildActionSpec(intentJson)

    /**
     * Open a thread by note ID. Rust registers `nmp.feed.thread.<noteId>` and
     * admits primary kind:1 notes plus derived repost wrappers for rendering.
     *
     * D6: null handle or invalid note_id is a silent no-op.
     */
    fun openThread(noteId: String) {
        if (handle != 0L) nativeOpenThread(handle, noteId)
    }

    /**
     * Close a thread feed opened with [openThread].
     */
    fun closeThread(noteId: String) {
        if (handle != 0L) nativeCloseThread(handle, noteId)
    }

    /**
     * Open an author profile by pubkey. Rust registers
     * `nmp.feed.author.<pubkey>` and admits primary kind:1 notes plus derived
     * repost wrappers for rendering. Profile metadata is fetched via [claimProfile].
     *
     * D6: null handle or invalid pubkey is a silent no-op.
     */
    fun openAuthor(pubkey: String) {
        if (handle != 0L) nativeOpenAuthor(handle, pubkey)
    }

    /** Close an author feed opened with [openAuthor]. */
    fun closeAuthor(pubkey: String) {
        if (handle != 0L) nativeCloseAuthor(handle, pubkey)
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

    /** Sign in with an nsec secret key (calls nmp_app_signin_nsec directly — no ActionModule for sign-in). */
    fun signInNsec(secret: String) {
        if (handle != 0L) nativeSignInNsec(handle, secret)
    }

    /** Sign in with a NIP-46 bunker URI through the Rust signer broker. */
    fun signInBunker(uri: String) {
        if (handle != 0L) nativeSignInBunker(handle, uri)
    }

    /** Cancel an in-flight NIP-46 handshake through the Rust signer broker. */
    fun cancelBunkerHandshake() {
        if (handle != 0L) nativeCancelBunkerHandshake(handle)
    }

    /**
     * ADR-0048 Stage 2 — begin a NIP-55 sign-in routed to `signerPackage`
     * (null = let the OS resolver pick). Rust builds the `get_public_key` +
     * permission-batch request and dispatches it through the capability
     * socket; the request is pushed to the registered
     * [KernelSignerRequestListener] (see [setSignerRequestListener]).
     */
    fun signInNip55(signerPackage: String?) {
        if (handle != 0L) nativeSignInNip55(handle, signerPackage)
    }

    /**
     * ADR-0048 Stage 2 / issue #1284 — register a push listener for outbound
     * NIP-55 capability requests (D8 — no polling; replaces the former
     * `nextSignerRequest` blocking drain).
     *
     * [listener] receives each `ExternalSignerRequest` JSON on the Rust
     * capability-dispatch thread — a native background thread, NOT the main
     * thread. The NIP-55 launch Intent requires the main thread, so the
     * implementation must marshal there itself. Replacing an existing listener
     * is allowed; pass a new one to swap.
     *
     * Call [clearSignerRequestListener] (or [closeUpdates], which clears it on
     * teardown) before [free]. D6: a null/dead handle is a no-op.
     */
    fun setSignerRequestListener(listener: KernelSignerRequestListener) {
        if (handle != 0L) nativeSetSignerRequestListener(handle, listener)
    }

    /**
     * Deregister the push listener set by [setSignerRequestListener]. Safe to
     * call when none is registered. D6: a null/dead handle is a no-op.
     */
    fun clearSignerRequestListener() {
        if (handle != 0L) nativeClearSignerRequestListener(handle)
    }

    /**
     * ADR-0048 Stage 2 — report a raw `ExternalSignerResponse` JSON back to
     * the Rust NIP-55 driver (D7: verbatim, Kotlin decides nothing).
     */
    fun deliverSignerResponse(responseJson: String) {
        if (handle != 0L) nativeDeliverSignerResponse(handle, responseJson)
    }

    /**
     * Generate a fresh `nostrconnect://` URI. Rust selects the relay from the
     * kernel's relay config (D3: relay selection is Rust-owned). Android
     * supplies only the optional platform callback scheme.
     */
    fun nostrConnectUri(callbackScheme: String? = null): String? =
        if (handle != 0L) nativeNostrConnectUri(handle, callbackScheme) else null

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
            nativeFree(current)
            handle = 0
        }
    }

    private external fun nativeNew(): Long
    private external fun nativeSetStoragePath(handle: Long, path: String): Int
    private external fun nativeStart(handle: Long, visibleLimit: Int, emitHz: Int)
    private external fun nativeOpenHomeFeed(handle: Long)
    private external fun nativeCreateLocalAccount(handle: Long, displayName: String)
    private external fun nativeStop(handle: Long)
    private external fun nativeClose(handle: Long)
    private external fun nativeLifecycleForeground(handle: Long)
    private external fun nativeLifecycleBackground(handle: Long)
    private external fun nativeIsAlive(handle: Long): Boolean
    private external fun nativeSetUpdateListener(handle: Long, listener: KernelUpdateListener)
    private external fun nativeClearUpdateListener(handle: Long)
    // Ref claim/release — `internal` so the cohesive ref-resolution wrappers live
    // in the sibling KernelBridgeRefs.kt without inflating this file past the LOC ceiling.
    internal external fun nativeClaimProfile(handle: Long, pubkey: String, consumerId: String)
    internal external fun nativeReleaseProfile(handle: Long, pubkey: String, consumerId: String)
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
    private external fun nativeDispatchAction(handle: Long, namespace: String, actionJson: String): String
    private external fun nativeAckActionStage(handle: Long, correlationId: String)
    // Outbox control-plane (parity GAP 4). `internal` so the cohesive
    // [retryPublish]/[cancelPublish] wrappers can live in the sibling
    // KernelBridgeOutboxRelay.kt without inflating this file past the LOC ceiling.
    internal external fun nativeRetryPublish(handle: Long, correlationId: String)
    internal external fun nativeCancelPublish(handle: Long, correlationId: String)
    private external fun nativeLoadOlderFeed(handle: Long, feedKey: String)
    private external fun nativeBuildActionSpec(intentJson: String): String
    private external fun nativeOpenThread(handle: Long, noteId: String)
    private external fun nativeCloseThread(handle: Long, noteId: String)
    private external fun nativeOpenAuthor(handle: Long, pubkey: String)
    private external fun nativeCloseAuthor(handle: Long, pubkey: String)
    private external fun nativeSeedRelays(handle: Long, relaysJson: String?)
    private external fun nativeAddRelay(handle: Long, url: String, role: String)
    private external fun nativeRemoveRelay(handle: Long, url: String)
    private external fun nativeSignInNsec(handle: Long, secret: String)
    private external fun nativeSignInBunker(handle: Long, uri: String)
    private external fun nativeCancelBunkerHandshake(handle: Long)
    private external fun nativeSignInNip55(handle: Long, signerPackage: String?)
    private external fun nativeSetSignerRequestListener(handle: Long, listener: KernelSignerRequestListener)
    private external fun nativeClearSignerRequestListener(handle: Long)
    private external fun nativeDeliverSignerResponse(handle: Long, responseJson: String)
    private external fun nativeNostrConnectUri(handle: Long, callbackScheme: String?): String?
    private external fun nativeSwitchAccount(handle: Long, pubkey: String)
    private external fun nativeRemoveAccount(handle: Long, pubkey: String)
    private external fun nativeMarmotRegisterActive(handle: Long, dbDir: String): Boolean
    private external fun nativeMarmotUnregister(handle: Long)
    private external fun nativeEncodeProfile(handle: Long, pubkey: String): String?
    private external fun nativeSetCapabilityHandler(handle: Long, handler: Any)
    private external fun nativeIdentityRestore(handle: Long, dbDir: String, testNsec: String?): Boolean
    private external fun nativeFree(handle: Long)
}
