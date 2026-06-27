package org.nmp.android

import android.util.Log
import java.util.UUID

private const val TAG = "SocialActions"

/**
 * Social write operations (NIP-25 reactions, NIP-57 zaps, NIP-18 reposts,
 * NIP-02 follow/unfollow, NIP-17 DMs) — Android peer of the iOS social action
 * surface. Extracted from [KernelModel] to keep both files under the repo's
 * 500-LOC hard ceiling (mirrors the [MarmotActions] extraction).
 *
 * M14-1 / PR2 (#2145): every social write is built by a generated
 * `GeneratedActionBuilders.*` FlatBuffers byte builder and dispatched through
 * the generic byte doorway via [dispatchBytes] — the retired JSON-intent path
 * (`ChirpActionIntent` → `dispatchIntentBytes`) is gone. App code NEVER spells a
 * namespace, assembles JSON, or builds a tag: Rust owns the protocol body, tag
 * construction, and validation. The host supplies only raw user input + a fresh
 * correlation id.
 *
 * Thin shell: ZERO protocol logic. Outcomes arrive reactively via the next
 * snapshot tick on [KernelModel.state] (D8 — no poll, no local echo).
 *
 * Call sites: [KernelModel] exposes one-line delegations (`model.zapNote(…)`
 * etc.) so the public surface is unchanged; the bodies live here.
 */
class SocialActions(
    private val dispatchBytes: (bytes: ByteArray) -> DispatchResult,
) {

    /**
     * Publish a new note (`nmp.nip01.publish_note`). Kotlin forwards only the
     * content + optional parent event id; Rust builds the kind:1 event and any
     * NIP-10 reply tags. Returns the correlation_id if accepted, or null on error.
     */
    fun publishNote(content: String, replyToId: String? = null): String? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.publishNote(
            correlationId = id,
            content = content,
            replyEventId = replyToId,
            replyAuthorPubkey = null,
            replyRootEventId = null,
            replyRootRelay = null,
            replyMentionedPubkeys = null,
        )
        val response = dispatchBuilt(bytes, "publishNote") ?: return null
        return response.correlationId
    }

    /** Zap a note (NIP-57). Relay selection is kernel policy — `relays` is empty. */
    fun zapNote(
        eventId: String,
        recipientPubkey: String,
        amountMsats: Long = 21000L,
        comment: String = "",
    ): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.zap(
            correlationId = id,
            recipientPubkey = recipientPubkey,
            amountMsats = amountMsats,
            lnurl = null,
            relays = emptyList(),
            targetEventId = eventId,
            comment = comment.takeIf { it.isNotEmpty() },
        )
        return dispatchBuilt(bytes, "zap")
    }

    /** React to a note (NIP-25). */
    fun react(eventId: String, reaction: String = "+"): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.react(
            correlationId = id,
            targetEventId = eventId,
            reaction = reaction,
            targetAuthorPubkey = null,
        )
        return dispatchBuilt(bytes, "react")
    }

    /** Repost a note (NIP-18 kind:6). Mirrors iOS `model.repost(eventID:authorPubkey:)`. */
    fun repost(eventId: String, authorPubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.repost(
            correlationId = id,
            eventId = eventId,
            authorPubkey = authorPubkey,
        )
        return dispatchBuilt(bytes, "repost")
    }

    /** Follow a pubkey. */
    fun follow(pubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.follow(correlationId = id, pubkey = pubkey)
        return dispatchBuilt(bytes, "follow")
    }

    /** Unfollow a pubkey. */
    fun unfollow(pubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.unfollow(correlationId = id, pubkey = pubkey)
        return dispatchBuilt(bytes, "unfollow")
    }

    /** Send a NIP-17 direct message to the given recipient pubkey. */
    fun sendDm(recipientPubkey: String, content: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        val bytes = GeneratedActionBuilders.sendDm(
            correlationId = id,
            recipientPubkey = recipientPubkey,
            content = content,
            replyTo = null,
        )
        return dispatchBuilt(bytes, "sendDm")
    }

    /**
     * Dispatch generated `DispatchEnvelope` bytes through the Rust byte doorway.
     * Returns null on a Rust-side rejection (fail-closed, D1/D6).
     */
    private fun dispatchBuilt(bytes: ByteArray, label: String): DispatchResult? {
        val response = dispatchBytes(bytes)
        Log.d(TAG, "dispatch($label) response: $response")
        return if (response is DispatchResult.Failure) null else response
    }
}
