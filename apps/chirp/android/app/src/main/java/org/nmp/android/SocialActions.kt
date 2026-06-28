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
 * Constructor takes one lambda owned by [KernelModel]:
 *  - [dispatchBytes] = `bridge.dispatchBytes` — dispatches a pre-encoded
 *    `DispatchEnvelope` FlatBuffers buffer through the typed byte doorway
 *    (M14-1 / #2145).
 *
 * Thin shell: ZERO protocol logic. Kotlin ferries typed user input into the
 * GENERATED [GeneratedActionBuilders] (ADR-0064 §3 — app code NEVER spells a
 * namespace or hand-assembles FlatBuffers; that lives only in generated code);
 * Rust owns action-namespace selection, body shape, tag construction, and
 * validation. Outcomes arrive reactively via the next snapshot tick on
 * [KernelModel.state] (D8 — no poll, no local echo).
 *
 * Call sites: [KernelModel] exposes one-line delegations (`model.zapNote(…)`
 * etc.) so the public surface is unchanged; the bodies live here.
 */
class SocialActions(
    private val dispatchBytes: (bytes: ByteArray) -> DispatchResult,
) {

    /**
     * Publish a new note. Kotlin forwards only user input; Rust builds the
     * `nmp.publish` namespace and body. A root note uses `publishRaw` (kind:1,
     * no tags); a reply uses `publishReply`, where Rust derives the NIP-10 tags
     * from the STORED parent event (a missing/invalid parent fails closed in
     * Rust and surfaces as a dispatch failure — D6). Returns the correlation_id
     * if accepted, or null on error.
     */
    fun publishNote(content: String, replyToId: String? = null): String? {
        val id = UUID.randomUUID().toString()
        val bytes = if (replyToId.isNullOrBlank()) {
            GeneratedActionBuilders.publishRaw(id, kind = 1, tags = emptyList(), content = content)
        } else {
            GeneratedActionBuilders.publishReply(id, content = content, replyToEventId = replyToId)
        }
        return (dispatch(bytes, "publishNote"))?.correlationId
    }

    /** Zap a note (NIP-57). */
    fun zapNote(
        eventId: String,
        recipientPubkey: String,
        amountMsats: Long = 21000L,
        comment: String = "",
    ): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(
            GeneratedActionBuilders.zap(
                correlationId = id,
                recipientPubkey = recipientPubkey,
                amountMsats = amountMsats,
                lnurl = null,
                relays = emptyList(),
                targetEventId = eventId,
                comment = comment.takeIf { it.isNotEmpty() },
            ),
            "zap",
        )
    }

    /** React to a note (NIP-25). */
    fun react(eventId: String, reaction: String = "+"): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(
            GeneratedActionBuilders.react(id, targetEventId = eventId, reaction = reaction, targetAuthorPubkey = null),
            "react",
        )
    }

    /** Repost a note (NIP-18 kind:6). Mirrors iOS `model.repost(eventID:authorPubkey:)`. */
    fun repost(eventId: String, authorPubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(
            GeneratedActionBuilders.repost(
                correlationId = id,
                targetEventId = eventId,
                targetKind = 1,
                targetAuthorPubkey = authorPubkey,
                relayHint = null,
            ),
            "repost",
        )
    }

    /** Follow a pubkey. */
    fun follow(pubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(GeneratedActionBuilders.follow(id, pubkey), "follow")
    }

    /** Unfollow a pubkey. */
    fun unfollow(pubkey: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(GeneratedActionBuilders.unfollow(id, pubkey), "unfollow")
    }

    /** Send a NIP-17 direct message to the given recipient pubkey. */
    fun sendDm(recipientPubkey: String, content: String): DispatchResult? {
        val id = UUID.randomUUID().toString()
        return dispatch(
            GeneratedActionBuilders.sendDm(id, recipientPubkey = recipientPubkey, content = content, replyTo = null),
            "sendDm",
        )
    }

    /**
     * Dispatch pre-encoded builder bytes through the Rust byte doorway. Returns
     * null on a Rust-side rejection (fail-closed, D1/D6).
     */
    private fun dispatch(bytes: ByteArray, op: String): DispatchResult? {
        val response = dispatchBytes(bytes)
        Log.d(TAG, "dispatch($op) response: $response")
        return if (response is DispatchResult.Failure) null else response
    }
}
