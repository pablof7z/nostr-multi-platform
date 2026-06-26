package org.nmp.android

/**
 * Signer sign-in and credential-delivery extension surface for [KernelBridge].
 *
 * Covers NIP-46 (bunker), NIP-55 (Amber/external signer), nsec, and the
 * NIP-55 signer-request push listener. Split out of [KernelBridge] to keep
 * that file under the 500-LOC ceiling (AGENTS.md §File-Size).
 *
 * All methods use [KernelBridge.rawHandle] so no private fields are accessed
 * across the extension boundary.
 */

/** Sign in with an nsec secret key. */
fun KernelBridge.signInNsec(secret: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeSignInNsec(handle, secret)
}

/** Sign in with a NIP-46 bunker URI through the Rust signer broker. */
fun KernelBridge.signInBunker(uri: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeSignInBunker(handle, uri)
}

/** Cancel an in-flight NIP-46 handshake through the Rust signer broker. */
fun KernelBridge.cancelBunkerHandshake() {
    val handle = rawHandle()
    if (handle != 0L) nativeCancelBunkerHandshake(handle)
}

/**
 * ADR-0048 Stage 2 — begin a NIP-55 sign-in routed to [signerPackage]
 * (null = let the OS resolver pick). Rust builds the `get_public_key` +
 * permission-batch request and dispatches it through the capability socket;
 * the request is pushed to the registered [KernelSignerRequestListener]
 * (see [setSignerRequestListener]).
 */
fun KernelBridge.signInNip55(signerPackage: String?) {
    val handle = rawHandle()
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
 * Call [clearSignerRequestListener] (or [KernelBridge.closeUpdates], which
 * clears it on teardown) before [KernelBridge.free]. D6: a null/dead handle
 * is a no-op.
 */
fun KernelBridge.setSignerRequestListener(listener: KernelSignerRequestListener) {
    val handle = rawHandle()
    if (handle != 0L) nativeSetSignerRequestListener(handle, listener)
}

/**
 * Deregister the push listener set by [setSignerRequestListener]. Safe to
 * call when none is registered. D6: a null/dead handle is a no-op.
 */
fun KernelBridge.clearSignerRequestListener() {
    val handle = rawHandle()
    if (handle != 0L) nativeClearSignerRequestListener(handle)
}

/**
 * ADR-0048 Stage 2 — report a raw `ExternalSignerResponse` JSON back to
 * the Rust NIP-55 driver (D7: verbatim, Kotlin decides nothing).
 */
fun KernelBridge.deliverSignerResponse(responseJson: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeDeliverSignerResponse(handle, responseJson)
}

/**
 * Generate a fresh `nostrconnect://` URI. Rust selects the relay from the
 * kernel's relay config (D3: relay selection is Rust-owned). Android
 * supplies only the optional platform callback scheme.
 */
fun KernelBridge.nostrConnectUri(callbackScheme: String? = null): String? {
    val handle = rawHandle()
    return if (handle != 0L) nativeNostrConnectUri(handle, callbackScheme) else null
}
