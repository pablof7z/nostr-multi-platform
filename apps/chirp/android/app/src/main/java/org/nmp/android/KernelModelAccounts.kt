package org.nmp.android

import org.nmp.android.model.RelayStatus

/**
 * Account-management and relay-management extension surface for [KernelModel].
 * Split out of [KernelModel] to keep that file under the 500-LOC ceiling
 * (AGENTS.md File Size). Same package — no import required. Public API surface
 * is unchanged. Thin-shell rule: no business logic here; all policy lives in
 * Rust (D7).
 */

// -------------------------------------------------------------------------
// Account management
// -------------------------------------------------------------------------

/** Sign in with an nsec secret key (direct C-ABI — no ActionModule for sign-in namespace).
 *
 *  No imperative post-identity `openHomeFeed()`: the view owns the typed feed
 *  session handle, and Rust owns active-follows source reduction/recompile on
 *  sign-in, switch, and logout. Driving feed repair from the identity op was a
 *  per-platform policy band-aid the shell must not carry (D7). */
fun KernelModel.signInNsec(secret: String) {
    bridge.signInNsec(secret)
}

/** Sign in with a NIP-46 bunker URI through the Rust signer broker. */
fun KernelModel.signInBunker(uri: String) {
    bridge.signInBunker(uri)
}

/** ADR-0048 Stage 2 — begin NIP-55 sign-in; Rust builds the get_public_key request. */
fun KernelModel.signInWithAmber(signer: NostrSignerInfo) {
    bridge.signInNip55(signer.packageName ?: signer.contentAuthority)
}

/** ADR-0048 Stage 2 — route ExternalSignerResponse JSON back to the Rust NIP-55 driver. */
fun KernelModel.deliverSignerResponse(responseJson: String) {
    bridge.deliverSignerResponse(responseJson)
}

fun KernelModel.cancelBunkerHandshake() {
    bridge.cancelBunkerHandshake()
}

fun KernelModel.nostrConnectUri(callbackScheme: String? = null): String? =
    bridge.nostrConnectUri(callbackScheme)

/** Create a new local account with the given display name.
 *  See [signInNsec] re: no imperative post-identity `openHomeFeed()`. */
fun KernelModel.createAccount(displayName: String) {
    bridge.createLocalAccount(displayName)
}

/** Switch the active account (direct C-ABI — no ActionModule for switch namespace).
 *  See [signInNsec] re: no imperative post-identity `openHomeFeed()`. */
fun KernelModel.switchAccount(pubkey: String) {
    bridge.switchAccount(pubkey)
}

/** Remove the account identified by the given pubkey (direct C-ABI). */
fun KernelModel.removeAccount(pubkey: String) = bridge.removeAccount(pubkey)

// -------------------------------------------------------------------------
// Relay management
// -------------------------------------------------------------------------

/** Add a relay with the given URL and role ("read", "write", or "both"). */
fun KernelModel.addRelay(url: String, role: String = "both") = bridge.addRelay(url, role)

/** Remove a relay by URL. */
fun KernelModel.removeRelay(url: String) = bridge.removeRelay(url)

/** Publish the current relay set as NIP-65 (kind:10002). #1291 GAP 5. */
fun KernelModel.publishRelayList(relays: List<RelayStatus>): DispatchResult =
    bridge.publishRelayList(relays)

/** Publish a NIP-17 DM relay-list (kind:10050) from `wss://` URLs. #1291 GAP 5. */
fun KernelModel.publishDmRelayList(relays: List<String>): DispatchResult =
    bridge.publishDmRelayList(relays)
