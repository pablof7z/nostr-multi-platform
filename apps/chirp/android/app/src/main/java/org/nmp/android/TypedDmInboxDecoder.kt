package org.nmp.android

import android.util.Log
import nmp.nip17.DmConversation as FbDmConversation
import nmp.nip17.DmInboxSnapshot as FbDmInboxSnapshot
import nmp.nip17.DmMessage as FbDmMessage
import org.nmp.android.model.DmConversation
import org.nmp.android.model.DmInboxSnapshot
import org.nmp.android.model.DmMessage
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "TypedDmInboxDecoder"

/**
 * Typed-first decoder for the NIP-17 `nmp.nip17.dm_inbox` snapshot projection
 * (`NDMI` / `DmInboxSnapshot`) — the Android peer of iOS `TypedDmInboxDecoder`
 * (`TypedProjectionDecoders.generated.swift`) + `TypedProjectionGlue.dmInbox`.
 *
 * Field-for-field mirror of `nmp_nip17::inbox::DmInboxSnapshot`: conversations
 * are newest-thread-first, messages oldest-first; the `has_reply_to` companion
 * bool reproduces the `Option<String> reply_to` null semantics. `source_relays`
 * is always a wire vector (the producer never omits it), mapped to the domain
 * `List<String>`.
 *
 * Returns `null` when the `NDMI` sidecar is absent / wrong schema /
 * unverifiable, so the typed-only host keeps the DM inbox projection absent. A
 * malformed sidecar yields `null` (fail closed, D1/D6).
 */
object TypedDmInboxDecoder {

    const val KEY = "nmp.nip17.dm_inbox"
    const val SCHEMA_ID = "nmp.nip17.dm_inbox"
    const val FILE_IDENTIFIER = "NDMI"

    fun decode(projections: List<TypedProjectionEnvelope>): DmInboxSnapshot? {
        val projection = projections.firstOrNull {
            it.key == KEY && it.schemaId == SCHEMA_ID
        } ?: return null
        if (projection.payload.isEmpty()) return null
        return decode(projection.payload)
    }

    /** Decode a raw `NDMI` buffer; `null` on any parse failure. */
    fun decode(bytes: ByteArray): DmInboxSnapshot? {
        if (bytes.isEmpty()) return null
        return try {
            val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
            if (!FbDmInboxSnapshot.DmInboxSnapshotBufferHasIdentifier(bb)) {
                Log.e(TAG, "NDMI file_identifier missing (${bytes.size} bytes)")
                return null
            }
            val snapshot = FbDmInboxSnapshot.getRootAsDmInboxSnapshot(bb)
            val conversations = buildList {
                for (i in 0 until snapshot.conversationsLength) {
                    val conv = snapshot.conversations(i) ?: continue
                    add(mapConversation(conv))
                }
            }
            DmInboxSnapshot(
                conversations = conversations,
                // §D7 — `decrypt_state` is a required string on the wire; an
                // absent / empty value maps to "unavailable" (the safe "host
                // hides the screen" default, never a misleading "ok").
                decryptState = snapshot.decryptState?.takeIf { it.isNotEmpty() } ?: "unavailable",
                undecryptedCount = snapshot.undecryptedCount,
            )
        } catch (e: Exception) {
            Log.e(TAG, "NDMI decode error: ${e.message} bytes=${bytes.size}")
            null
        }
    }

    private fun mapConversation(conv: FbDmConversation): DmConversation {
        val messages = buildList {
            for (i in 0 until conv.messagesLength) {
                val msg = conv.messages(i) ?: continue
                add(mapMessage(msg))
            }
        }
        return DmConversation(
            peerPubkey = conv.peerPubkey ?: "",
            messages = messages,
        )
    }

    private fun mapMessage(msg: FbDmMessage): DmMessage {
        val sourceRelays = buildList {
            for (i in 0 until msg.sourceRelaysLength) {
                add(msg.sourceRelays(i) ?: continue)
            }
        }
        return DmMessage(
            id = msg.id ?: "",
            senderPubkey = msg.senderPubkey ?: "",
            content = msg.content ?: "",
            createdAt = msg.createdAt.toLong(),
            // has_reply_to == false mirrors the generic JSON `null`.
            replyTo = if (msg.hasReplyTo) msg.replyTo else null,
            isOutgoing = msg.isOutgoing,
            sourceRelays = sourceRelays,
        )
    }
}
