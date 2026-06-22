// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform kotlin \
//       --out android/app/src/main/java/org/nmp/android/ActionBuilders.kt
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated Kotlin differs from a fresh run.
//
// ADR-0064 §3 — typed write builders. Each function below encodes the per-crate
// FlatBuffers payload for one open-registry `action_namespace` and stamps it,
// the namespace, and the envelope schema_version into a `DispatchEnvelope`,
// returning the finished bytes for the native byte doorway
// `nmp_app_dispatch_action_bytes` (#1752). App code NEVER spells a namespace
// string or hand-assembles FlatBuffers — that lives only here, in generated
// code. The host supplies the `correlationId` (the operation identity end to
// end, ADR-0064 §4) and owns the JNI call.
// ─────────────────────────────────────────────────────────────────────────────

package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder

object GeneratedActionBuilders {
    /// The single recognised envelope schema version — mirrors
    /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.
    const val DISPATCH_ENVELOPE_SCHEMA_VERSION: Int = 1

    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a
    /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).
    /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.
    private fun encodeDispatchEnvelope(
        correlationId: String,
        actionNamespace: String,
        payload: ByteArray,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val correlationOffset = fbb.createString(correlationId)
        val namespaceOffset = fbb.createString(actionNamespace)
        val payloadOffset = fbb.createByteVector(payload)
        fbb.startTable(4)
        fbb.addOffset(0, correlationOffset, 0)   // slot 0: correlation_id
        fbb.addOffset(1, namespaceOffset, 0)     // slot 1: action_namespace
        fbb.addInt(2, DISPATCH_ENVELOPE_SCHEMA_VERSION, 0) // slot 2: schema_version
        fbb.addOffset(3, payloadOffset, 0)       // slot 3: payload
        val root = fbb.endTable()
        fbb.finish(root, "NMPD")
        return fbb.sizedByteArray()
    }

    /// Publish a NIP-25 reaction to a target event.
    /// Builds the `nmp.nip25.react` `DispatchEnvelope` bytes for the byte doorway.
    fun react(
        correlationId: String,
        targetEventId: String,
        reaction: String,
        targetAuthorPubkey: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val targetEventIdOffset = fbb.createString(targetEventId)
        val reactionOffset = fbb.createString(reaction)
        val targetAuthorPubkeyOffset = targetAuthorPubkey?.let { fbb.createString(it) } ?: 0
        fbb.startTable(4)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, targetEventIdOffset, 0) // slot 1: targetEventId
        fbb.addOffset(2, reactionOffset, 0) // slot 2: reaction
        if (targetAuthorPubkeyOffset != 0) fbb.addOffset(3, targetAuthorPubkeyOffset, 0) // slot 3: targetAuthorPubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N25R")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip25.react",
            payload = payload,
        )
    }

    /// Retract a previously-published NIP-25 reaction.
    /// Builds the `nmp.nip25.unreact` `DispatchEnvelope` bytes for the byte doorway.
    fun unreact(
        correlationId: String,
        reactionEventId: String,
        reason: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val reactionEventIdOffset = fbb.createString(reactionEventId)
        val reasonOffset = fbb.createString(reason)
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, reactionEventIdOffset, 0) // slot 1: reactionEventId
        fbb.addOffset(2, reasonOffset, 0) // slot 2: reason
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N25U")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip25.unreact",
            payload = payload,
        )
    }

    /// Follow a single pubkey (NIP-02 contact-list add).
    /// Builds the `nmp.follow` `DispatchEnvelope` bytes for the byte doorway.
    fun follow(
        correlationId: String,
        pubkey: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val pubkeyOffset = fbb.createString(pubkey)
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, pubkeyOffset, 0) // slot 1: pubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NF2A")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.follow",
            payload = payload,
        )
    }

    /// Unfollow a single pubkey (NIP-02 contact-list remove).
    /// Builds the `nmp.unfollow` `DispatchEnvelope` bytes for the byte doorway.
    fun unfollow(
        correlationId: String,
        pubkey: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val pubkeyOffset = fbb.createString(pubkey)
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, pubkeyOffset, 0) // slot 1: pubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NF2A")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.unfollow",
            payload = payload,
        )
    }

    /// Follow many pubkeys in one race-free read-modify-write cycle (NIP-02).
    /// Builds the `nmp.follow_many` `DispatchEnvelope` bytes for the byte doorway.
    fun followMany(
        correlationId: String,
        pubkeys: List<String>?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val pubkeysOffset = run {
            val values = pubkeys
            if (values == null || values.isEmpty()) 0 else {
                val offsets = IntArray(values.size) { i -> fbb.createString(values[i]) }
                fbb.startVector(4, offsets.size, 4)
                for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
                fbb.endVector()
            }
        }
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        if (pubkeysOffset != 0) fbb.addOffset(1, pubkeysOffset, 0) // slot 1: pubkeys
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NFMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.follow_many",
            payload = payload,
        )
    }
}
