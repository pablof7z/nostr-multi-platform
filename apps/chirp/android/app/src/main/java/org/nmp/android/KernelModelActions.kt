package org.nmp.android

/**
 * Outbox control-plane, wallet (NIP-47/NWC), and social write-op extension
 * surface for [KernelModel]. Split out of [KernelModel] to keep that file under
 * the 500-LOC ceiling (AGENTS.md File Size). Same package — no import required.
 * Public API surface is typed; namespace/body dispatch stays below
 * [KernelBridge].
 */

// -------------------------------------------------------------------------
// Outbox control-plane
// -------------------------------------------------------------------------

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

/** Connect a NIP-47 wallet via NWC URI. */
fun KernelModel.dispatchWalletConnect(uri: String): DispatchResult = bridge.walletConnect(uri)

/** Disconnect the current NIP-47 wallet. */
fun KernelModel.dispatchWalletDisconnect(): DispatchResult = bridge.walletDisconnect()

/** Pay a Lightning invoice via the NIP-47 wallet. [amountMsats] is `null` for
 *  self-specified (amountless) invoices; a non-null value overrides the invoice
 *  amount (used when the invoice has no embedded amount).
 */
fun KernelModel.dispatchWalletPayInvoice(bolt11: String, amountMsats: Long? = null): DispatchResult =
    bridge.walletPayInvoice(bolt11, amountMsats)

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
