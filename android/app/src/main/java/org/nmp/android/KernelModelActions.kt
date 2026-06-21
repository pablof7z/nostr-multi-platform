package org.nmp.android

import android.util.Log

private const val TAG = "NmpCore"

/**
 * Action-dispatch, outbox control-plane, wallet (NIP-47/NWC), and social write-op
 * extension surface for [KernelModel]. Split out of [KernelModel] to keep that
 * file under the 500-LOC ceiling (AGENTS.md File Size). Same package — no import
 * required. Public API surface is unchanged. Thin-shell rule: no business logic
 * here; Rust owns all dispatch policy (D7).
 */

// -------------------------------------------------------------------------
// Generic action dispatch + outbox control-plane
// -------------------------------------------------------------------------

/**
 * Dispatch a named action through the action registry (generic path).
 * Fire-and-forget — outcomes arrive in the next snapshot tick.
 */
fun KernelModel.dispatchAction(namespace: String, actionJson: String): DispatchResult {
    val result = bridge.dispatchAction(namespace, actionJson)
    Log.d(TAG, "dispatchAction($namespace) response: $result")
    return result
}

fun KernelModel.ackActionStage(correlationId: String) {
    bridge.ackActionStage(correlationId)
}

/** Retry a failed publish from the outbox (#1291 GAP 4). */
fun KernelModel.retryPublish(correlationId: String) {
    bridge.retryPublish(correlationId)
}

/** Cancel an in-flight publish from the outbox (#1291 GAP 4). */
fun KernelModel.cancelPublish(correlationId: String) {
    bridge.cancelPublish(correlationId)
}

// -------------------------------------------------------------------------
// Wallet (NIP-47 / NWC)
// -------------------------------------------------------------------------

/** Connect a NIP-47 wallet via NWC URI. [actionJson] = {"Connect":{"uri":"nostr+walletconnect://..."}} */
fun KernelModel.dispatchWalletConnect(actionJson: String) {
    val response = bridge.dispatchAction("nmp.wallet.connect", actionJson)
    Log.d(TAG, "wallet connect response: $response")
}

/** Disconnect the current NIP-47 wallet. */
fun KernelModel.dispatchWalletDisconnect() {
    val response = bridge.dispatchAction("nmp.wallet.disconnect", "\"Disconnect\"")
    Log.d(TAG, "wallet disconnect response: $response")
}

// -------------------------------------------------------------------------
// Social + DM — write ops live in [social: SocialActions]; these delegate so
// the public surface (model.zapNote(…) etc.) is unchanged.
// -------------------------------------------------------------------------

/** Zap a note (NIP-57). */
fun KernelModel.zapNote(
    eventId: String,
    recipientPubkey: String,
    amountMsats: Long = 21000L,
    comment: String = "",
): DispatchResult? = social.zapNote(eventId, recipientPubkey, amountMsats, comment)

/** React to a note (NIP-25). */
fun KernelModel.react(eventId: String, reaction: String = "+"): DispatchResult? =
    social.react(eventId, reaction)

/** Repost a note (NIP-18 kind:6). Mirrors iOS `model.repost(eventID:authorPubkey:)`. */
fun KernelModel.repost(eventId: String, authorPubkey: String): DispatchResult? =
    social.repost(eventId, authorPubkey)

/** Follow a pubkey. */
fun KernelModel.follow(pubkey: String): DispatchResult? = social.follow(pubkey)

/** Unfollow a pubkey. */
fun KernelModel.unfollow(pubkey: String): DispatchResult? = social.unfollow(pubkey)

/** Send a NIP-17 direct message to the given recipient pubkey. */
fun KernelModel.sendDm(recipientPubkey: String, content: String): DispatchResult? =
    social.sendDm(recipientPubkey, content)
