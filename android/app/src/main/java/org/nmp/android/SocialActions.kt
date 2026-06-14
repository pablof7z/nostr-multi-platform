package org.nmp.android

import android.util.Log
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString

private const val TAG = "SocialActions"

/**
 * Social write operations (NIP-25 reactions, NIP-57 zaps, NIP-18 reposts,
 * NIP-02 follow/unfollow, NIP-17 DMs) — Android peer of the iOS social action
 * surface. Extracted from [KernelModel] to keep both files under the repo's
 * 500-LOC hard ceiling (mirrors the [MarmotActions] extraction).
 *
 * Constructor takes two lambdas owned by [KernelModel]:
 *  - [buildActionSpec] = `bridge.buildActionSpec` — Rust builds the namespace +
 *    body JSON from typed user intent.
 *  - [dispatchAction] = `bridge.dispatchAction` — the generic action dispatch.
 *
 * Thin shell: ZERO protocol logic. Kotlin ferries typed user intent; Rust owns
 * action-namespace selection, body shape, tag construction, and validation.
 * Outcomes arrive reactively via the next snapshot tick on [KernelModel.state]
 * (D8 — no poll, no local echo).
 *
 * Call sites: [KernelModel] exposes one-line delegations (`model.zapNote(…)`
 * etc.) so the public surface is unchanged; the bodies live here.
 */
class SocialActions(
    private val buildActionSpec: (intentJson: String) -> String,
    private val dispatchAction: (namespace: String, actionJson: String) -> DispatchResult,
) {

    /**
     * Publish a new note. Kotlin forwards only user intent; Rust builds the
     * `nmp.publish` namespace and `PublishRaw` body, including reply tags.
     * Returns the correlation_id if accepted, or null on error.
     */
    fun publishNote(content: String, replyToId: String? = null): String? {
        val response = dispatchTypedIntent(
            ChirpActionIntent(
                type = "publish_note",
                content = content,
                replyToEventId = replyToId,
            )
        ) ?: return null
        return response.correlationId
    }

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

    /** Repost a note (NIP-18 kind:6). Mirrors iOS `model.repost(eventID:authorPubkey:)`. */
    fun repost(eventId: String, authorPubkey: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "repost", eventId = eventId, authorPubkey = authorPubkey)
    )

    /** Follow a pubkey. */
    fun follow(pubkey: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "follow", pubkey = pubkey)
    )

    /** Unfollow a pubkey. */
    fun unfollow(pubkey: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "unfollow", pubkey = pubkey)
    )

    /** Send a NIP-17 direct message to the given recipient pubkey. */
    fun sendDm(recipientPubkey: String, content: String): DispatchResult? = dispatchTypedIntent(
        ChirpActionIntent(type = "send_dm", recipientPubkey = recipientPubkey, content = content)
    )

    /**
     * Build a Chirp action spec from typed user intent (Rust owns the namespace
     * and body shape), then dispatch it. Returns null on parse error, Rust-side
     * rejection, or missing dispatch fields (fail-closed, D1/D6).
     */
    private fun dispatchTypedIntent(intent: ChirpActionIntent): DispatchResult? {
        val intentJson = chirpActionJson.encodeToString(intent)
        val specResponse = buildActionSpec(intentJson)
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
        val response = dispatchAction(spec.namespace, spec.bodyJson)
        Log.d(TAG, "dispatchTypedIntent(${intent.type}) response: $response")
        return response
    }
}
