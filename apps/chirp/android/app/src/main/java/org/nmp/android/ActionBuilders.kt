// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform kotlin \
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/ActionBuilders.kt
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

    /// Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
    /// mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
    /// Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
    /// so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
    /// Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive.
    private fun relayMarkerByte(role: String): Byte {
        var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false
        var invalid = false
        for (part in role.split(",").map { it.trim().lowercase() }) {
            when (part) {
                "" -> {}
                "both" -> hasBoth = true
                "read" -> hasRead = true
                "write" -> hasWrite = true
                "indexer" -> hasIndexer = true
                else -> invalid = true
            }
        }
        if (invalid) return 255.toByte()
        return (when {
            hasBoth || (hasRead && hasWrite) -> 0
            hasRead -> 1
            hasWrite -> 2
            hasIndexer -> 3
            else -> 255
        }).toByte()
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

    /// Publish a NIP-18 repost wrapper for a target event.
    /// Builds the `nmp.nip18.repost` `DispatchEnvelope` bytes for the byte doorway.
    fun repost(
        correlationId: String,
        targetEventId: String,
        targetKind: Int,
        targetAuthorPubkey: String?,
        relayHint: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val targetEventIdOffset = fbb.createString(targetEventId)
        val targetAuthorPubkeyOffset = targetAuthorPubkey?.let { fbb.createString(it) } ?: 0
        val relayHintOffset = relayHint?.let { fbb.createString(it) } ?: 0
        fbb.startTable(5)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, targetEventIdOffset, 0) // slot 1: targetEventId
        fbb.addInt(2, targetKind, 0) // slot 2: targetKind
        if (targetAuthorPubkeyOffset != 0) fbb.addOffset(3, targetAuthorPubkeyOffset, 0) // slot 3: targetAuthorPubkey
        if (relayHintOffset != 0) fbb.addOffset(4, relayHintOffset, 0) // slot 4: relayHint
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N18R")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip18.repost",
            payload = payload,
        )
    }

    /// Publish a NIP-18 quote repost note for a target event.
    /// Builds the `nmp.nip18.quote_repost` `DispatchEnvelope` bytes for the byte doorway.
    fun quoteRepost(
        correlationId: String,
        targetEventId: String,
        targetKind: Int,
        targetAuthorPubkey: String?,
        relayHint: String?,
        content: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val targetEventIdOffset = fbb.createString(targetEventId)
        val targetAuthorPubkeyOffset = targetAuthorPubkey?.let { fbb.createString(it) } ?: 0
        val relayHintOffset = relayHint?.let { fbb.createString(it) } ?: 0
        val contentOffset = fbb.createString(content)
        fbb.startTable(6)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, targetEventIdOffset, 0) // slot 1: targetEventId
        fbb.addInt(2, targetKind, 0) // slot 2: targetKind
        if (targetAuthorPubkeyOffset != 0) fbb.addOffset(3, targetAuthorPubkeyOffset, 0) // slot 3: targetAuthorPubkey
        if (relayHintOffset != 0) fbb.addOffset(4, relayHintOffset, 0) // slot 4: relayHint
        fbb.addOffset(5, contentOffset, 0) // slot 5: content
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N18Q")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip18.quote_repost",
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

    /// Add one item to the active account's NIP-51 bookmark list.
    /// Builds the `nmp.nip51.add_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    fun addBookmark(
        correlationId: String,
        accountPubkey: String,
        itemKind: Int,
        value: String,
        relay: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        val valueOffset = fbb.createString(value)
        val relayOffset = relay?.let { fbb.createString(it) } ?: 0
        fbb.startTable(3)
        fbb.addByte(0, itemKind.toByte(), 0) // slot 0: kind
        fbb.addOffset(1, valueOffset, 0) // slot 1: value
        if (relayOffset != 0) fbb.addOffset(2, relayOffset, 0) // slot 2: relay
        val itemRoot = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: account_pubkey
        fbb.addOffset(2, itemRoot, 0) // slot 2: item
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N51B")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.add_bookmark",
            payload = payload,
        )
    }

    /// Remove one item from the active account's NIP-51 bookmark list.
    /// Builds the `nmp.nip51.remove_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    fun removeBookmark(
        correlationId: String,
        accountPubkey: String,
        itemKind: Int,
        value: String,
        relay: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        val valueOffset = fbb.createString(value)
        val relayOffset = relay?.let { fbb.createString(it) } ?: 0
        fbb.startTable(3)
        fbb.addByte(0, itemKind.toByte(), 0) // slot 0: kind
        fbb.addOffset(1, valueOffset, 0) // slot 1: value
        if (relayOffset != 0) fbb.addOffset(2, relayOffset, 0) // slot 2: relay
        val itemRoot = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: account_pubkey
        fbb.addOffset(2, itemRoot, 0) // slot 2: item
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N51B")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.remove_bookmark",
            payload = payload,
        )
    }

    /// Add a relay URL to the NIP-51 blocked-relay list.
    /// Builds the `nmp.nip51.block_relay` `DispatchEnvelope` bytes for the byte doorway.
    fun blockRelay(
        correlationId: String,
        url: String,
        accountPubkey: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val urlOffset = fbb.createString(url)
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, urlOffset, 0) // slot 1: url
        fbb.addOffset(2, accountPubkeyOffset, 0) // slot 2: accountPubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NBLK")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.block_relay",
            payload = payload,
        )
    }

    /// Remove a relay URL from the NIP-51 blocked-relay list.
    /// Builds the `nmp.nip51.unblock_relay` `DispatchEnvelope` bytes for the byte doorway.
    fun unblockRelay(
        correlationId: String,
        url: String,
        accountPubkey: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val urlOffset = fbb.createString(url)
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, urlOffset, 0) // slot 1: url
        fbb.addOffset(2, accountPubkeyOffset, 0) // slot 2: accountPubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NUBL")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.unblock_relay",
            payload = payload,
        )
    }

    /// Publish a NIP-17 DM relay list (kind:10050).
    /// Builds the `nmp.nip17.publish_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    fun publishDmRelayList(
        correlationId: String,
        relays: List<String>,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relaysOffset = run {
            val offsets = IntArray(relays.size) { i -> fbb.createString(relays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, relaysOffset, 0) // slot 1: relays
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N17R")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip17.publish_relay_list",
            payload = payload,
        )
    }

    /// Hydrate a DM peer's NIP-17 relay list (kind:10050).
    /// Builds the `nmp.nip17.hydrate_peer_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    fun hydrateDmPeerRelayList(
        correlationId: String,
        peerPubkey: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val peerPubkeyOffset = fbb.createString(peerPubkey)
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, peerPubkeyOffset, 0) // slot 1: peerPubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N17H")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip17.hydrate_peer_relay_list",
            payload = payload,
        )
    }

    /// Publish a NIP-65 relay-list metadata event (kind:10002).
    /// Builds the `nmp.nip65.publish_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    fun publishRelayList(
        correlationId: String,
        relays: List<Pair<String, String>>,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relaysOffset = run {
            val entryOffsets = IntArray(relays.size) { i ->
                val (url, role) = relays[i]
                val urlOff = fbb.createString(url)
                fbb.startTable(2)
                fbb.addOffset(0, urlOff, 0) // RelayListEntry slot 0: url
                fbb.addByte(1, relayMarkerByte(role), 0) // RelayListEntry slot 1: marker
                fbb.endTable()
            }
            fbb.startVector(4, entryOffsets.size, 4)
            for (i in entryOffsets.size - 1 downTo 0) fbb.addOffset(entryOffsets[i])
            fbb.endVector()
        }
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, relaysOffset, 0) // slot 1: relays
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N65P")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip65.publish_relay_list",
            payload = payload,
        )
    }

    /// Connect a NIP-47 Nostr Wallet Connect URI.
    /// Builds the `nmp.wallet.connect` `DispatchEnvelope` bytes for the byte doorway.
    fun walletConnect(
        correlationId: String,
        uri: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val uriOffset = fbb.createString(uri)
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, uriOffset, 0) // slot 1: uri
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N47C")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.wallet.connect",
            payload = payload,
        )
    }

    /// Disconnect the current NIP-47 wallet (no payload data beyond schema_version).
    /// Builds the `nmp.wallet.disconnect` `DispatchEnvelope` bytes for the byte doorway.
    fun walletDisconnect(
        correlationId: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        fbb.startTable(1)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N47D")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.wallet.disconnect",
            payload = payload,
        )
    }

    /// Pay a Lightning invoice via the NIP-47 wallet.
    /// Builds the `nmp.wallet.pay_invoice` `DispatchEnvelope` bytes for the byte doorway.
    fun walletPayInvoice(
        correlationId: String,
        bolt11: String,
        amountMsats: Long?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val bolt11Offset = fbb.createString(bolt11)
        fbb.startTable(4)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, bolt11Offset, 0) // slot 1: bolt11
        if (amountMsats != null) {
            fbb.addLong(2, amountMsats, 0L) // slot 2: amountMsats
            fbb.addBoolean(3, true, false) // slot 3: hasAmountMsats
        }
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N47P")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.wallet.pay_invoice",
            payload = payload,
        )
    }

    /// Send a NIP-17 gift-wrapped direct message to a recipient.
    /// Builds the `nmp.nip17.send` `DispatchEnvelope` bytes for the byte doorway.
    fun sendDm(
        correlationId: String,
        recipientPubkey: String,
        content: String,
        replyTo: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val recipientPubkeyOffset = fbb.createString(recipientPubkey)
        val contentOffset = fbb.createString(content)
        val replyToOffset = replyTo?.let { fbb.createString(it) } ?: 0
        fbb.startTable(4)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, recipientPubkeyOffset, 0) // slot 1: recipientPubkey
        fbb.addOffset(2, contentOffset, 0) // slot 2: content
        if (replyToOffset != 0) fbb.addOffset(3, replyToOffset, 0) // slot 3: replyTo
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N17S")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip17.send",
            payload = payload,
        )
    }

    /// Publish a NIP-57 zap request for a recipient (optionally a target event).
    /// Builds the `nmp.nip57.zap` `DispatchEnvelope` bytes for the byte doorway.
    fun zap(
        correlationId: String,
        recipientPubkey: String,
        amountMsats: Long,
        lnurl: String?,
        relays: List<String>,
        targetEventId: String?,
        comment: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val recipientPubkeyOffset = fbb.createString(recipientPubkey)
        val lnurlOffset = lnurl?.let { fbb.createString(it) } ?: 0
        val relaysOffset = run {
            val offsets = IntArray(relays.size) { i -> fbb.createString(relays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        val targetEventIdOffset = targetEventId?.let { fbb.createString(it) } ?: 0
        val commentOffset = comment?.let { fbb.createString(it) } ?: 0
        fbb.startTable(7)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, recipientPubkeyOffset, 0) // slot 1: recipientPubkey
        fbb.addLong(2, amountMsats, 0L) // slot 2: amountMsats
        if (lnurlOffset != 0) fbb.addOffset(3, lnurlOffset, 0) // slot 3: lnurl
        fbb.addOffset(4, relaysOffset, 0) // slot 4: relays
        if (targetEventIdOffset != 0) fbb.addOffset(5, targetEventIdOffset, 0) // slot 5: targetEventId
        if (commentOffset != 0) fbb.addOffset(6, commentOffset, 0) // slot 6: comment
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N57Z")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip57.zap",
            payload = payload,
        )
    }

    /// Sign-and-publish an arbitrary event kind (generic publish path; NIP-65 outbox or explicit relays).
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishRaw`) for the byte doorway.
    fun publishRaw(
        correlationId: String,
        kind: Int,
        tags: List<List<String>>,
        content: String,
        relays: List<String>? = null,
        signerPubkey: String? = null,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val tagRowOffsets = IntArray(tags.size) { r ->
            val row = tags[r]
            val valueOffsets = IntArray(row.size) { i -> fbb.createString(row[i]) }
            fbb.startVector(4, valueOffsets.size, 4)
            for (i in valueOffsets.size - 1 downTo 0) fbb.addOffset(valueOffsets[i])
            val valuesVec = fbb.endVector()
            fbb.startTable(1)
            fbb.addOffset(0, valuesVec, 0) // slot 0: values
            fbb.endTable()
        }
        val tagsVec = run {
            fbb.startVector(4, tagRowOffsets.size, 4)
            for (i in tagRowOffsets.size - 1 downTo 0) fbb.addOffset(tagRowOffsets[i])
            fbb.endVector()
        }
        val contentOffset = fbb.createString(content)
        val signerPubkeyOffset = signerPubkey?.let { fbb.createString(it) } ?: 0
        val targetRelays = relays ?: emptyList()
        val explicit = targetRelays.isNotEmpty()
        val targetRelaysVec = run {
            val offsets = IntArray(targetRelays.size) { i -> fbb.createString(targetRelays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        fbb.startTable(2)
        fbb.addBoolean(0, explicit, false) // slot 0: explicit
        fbb.addOffset(1, targetRelaysVec, 0) // slot 1: relays
        val targetOffset = fbb.endTable()
        fbb.startTable(5)
        fbb.addInt(0, kind, 0) // slot 0: kind
        fbb.addOffset(1, tagsVec, 0) // slot 1: tags
        fbb.addOffset(2, contentOffset, 0) // slot 2: content
        fbb.addOffset(3, targetOffset, 0) // slot 3: target
        if (signerPubkeyOffset != 0) fbb.addOffset(4, signerPubkeyOffset, 0) // slot 4: signer_pubkey
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 3.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NPUB")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.publish",
            payload = payload,
        )
    }

    /// Sign-and-publish a kind:1 reply; Rust derives NIP-10 tags from the stored parent event.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishReply`) for the byte doorway.
    fun publishReply(
        correlationId: String,
        content: String,
        replyToEventId: String,
        relays: List<String>? = null,
        signerPubkey: String? = null,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val contentOffset = fbb.createString(content)
        val replyToEventIdOffset = fbb.createString(replyToEventId)
        val signerPubkeyOffset = signerPubkey?.let { fbb.createString(it) } ?: 0
        val targetRelays = relays ?: emptyList()
        val explicit = targetRelays.isNotEmpty()
        val targetRelaysVec = run {
            val offsets = IntArray(targetRelays.size) { i -> fbb.createString(targetRelays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        fbb.startTable(2)
        fbb.addBoolean(0, explicit, false) // slot 0: explicit
        fbb.addOffset(1, targetRelaysVec, 0) // slot 1: relays
        val targetOffset = fbb.endTable()
        fbb.startTable(4)
        fbb.addOffset(0, contentOffset, 0) // slot 0: content
        fbb.addOffset(1, replyToEventIdOffset, 0) // slot 1: reply_to_event_id
        fbb.addOffset(2, targetOffset, 0) // slot 2: target
        if (signerPubkeyOffset != 0) fbb.addOffset(3, signerPubkeyOffset, 0) // slot 3: signer_pubkey
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 4.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NPUB")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.publish",
            payload = payload,
        )
    }

    /// Sign-and-publish a kind:0 profile metadata event for the active account.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishProfile`) for the byte doorway.
    fun publishProfile(
        correlationId: String,
        fields: List<Pair<String, String>>,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val profileFieldOffsets = IntArray(fields.size) { i ->
            val keyOffset = fbb.createString(fields[i].first)
            val valueOffset = fbb.createString(fields[i].second)
            fbb.startTable(2)
            fbb.addOffset(0, keyOffset, 0) // slot 0: key
            fbb.addOffset(1, valueOffset, 0) // slot 1: value
            fbb.endTable()
        }
        val fieldsVec = run {
            fbb.startVector(4, profileFieldOffsets.size, 4)
            for (i in profileFieldOffsets.size - 1 downTo 0) fbb.addOffset(profileFieldOffsets[i])
            fbb.endVector()
        }
        fbb.startTable(1)
        fbb.addOffset(0, fieldsVec, 0) // slot 0: fields
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 2.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NPUB")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.publish",
            payload = payload,
        )
    }

    /// Publish (or rotate) the local MLS key-package (kind:30443) to relays.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `PublishKeyPackage`) for the byte doorway.
    fun marmotPublishKeyPackage(
        correlationId: String,
        relays: List<String> = emptyList(),
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relaysVec = run {
            val offs = IntArray(relays.size) { i -> fbb.createString(relays[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        }
        fbb.startTable(1)
        fbb.addOffset(0, relaysVec, 0) // slot 0: relays
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 1.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Create a new MLS group and optionally invite peers.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `CreateGroup`) for the byte doorway.
    fun marmotCreateGroup(
        correlationId: String,
        name: String,
        description: String = "",
        inviteeText: String? = null,
        inviteeNpubs: List<String>? = null,
        signedKeyPackageEventsJson: List<String> = emptyList(),
        relays: List<String> = emptyList(),
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relaysVec = run {
            val offs = IntArray(relays.size) { i -> fbb.createString(relays[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        }
        val jsonVec = run {
            val offs = IntArray(signedKeyPackageEventsJson.size) { i -> fbb.createString(signedKeyPackageEventsJson[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        }
        // inviteeNpubs: null → absent (None); non-null → present vector (even if empty)
        val npubsVec = inviteeNpubs?.let { npubs ->
            val offs = IntArray(npubs.size) { i -> fbb.createString(npubs[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        } ?: 0
        val inviteeTextOffset = inviteeText?.let { fbb.createString(it) } ?: 0
        val descOffset = if (description.isEmpty()) 0 else fbb.createString(description)
        val nameOffset = fbb.createString(name)
        fbb.startTable(6)
        fbb.addOffset(0, nameOffset, 0) // slot 0: name (required)
        if (descOffset != 0) fbb.addOffset(1, descOffset, 0) // slot 1: description
        if (inviteeTextOffset != 0) fbb.addOffset(2, inviteeTextOffset, 0) // slot 2: invitee_text
        if (npubsVec != 0) fbb.addOffset(3, npubsVec, 0) // slot 3: invitee_npubs
        fbb.addOffset(4, jsonVec, 0) // slot 4: signed_key_package_events_json
        fbb.addOffset(5, relaysVec, 0) // slot 5: relays
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 2.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Invite one or more peers to an existing MLS group.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Invite`) for the byte doorway.
    fun marmotInvite(
        correlationId: String,
        groupIdHex: String,
        inviteeText: String? = null,
        inviteeNpubs: List<String>? = null,
        signedKeyPackageEventsJson: List<String> = emptyList(),
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val jsonVec = run {
            val offs = IntArray(signedKeyPackageEventsJson.size) { i -> fbb.createString(signedKeyPackageEventsJson[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        }
        val npubsVec = inviteeNpubs?.let { npubs ->
            val offs = IntArray(npubs.size) { i -> fbb.createString(npubs[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        } ?: 0
        val inviteeTextOffset = inviteeText?.let { fbb.createString(it) } ?: 0
        val gidOffset = fbb.createString(groupIdHex)
        fbb.startTable(4)
        fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)
        if (inviteeTextOffset != 0) fbb.addOffset(1, inviteeTextOffset, 0) // slot 1: invitee_text
        if (npubsVec != 0) fbb.addOffset(2, npubsVec, 0) // slot 2: invitee_npubs
        fbb.addOffset(3, jsonVec, 0) // slot 3: signed_key_package_events_json
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 3.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Send a kind:14 NIP-44 MLS group message.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Send`) for the byte doorway.
    fun marmotSend(
        correlationId: String,
        groupIdHex: String,
        text: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val textOffset = fbb.createString(text)
        val gidOffset = fbb.createString(groupIdHex)
        fbb.startTable(2)
        fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)
        fbb.addOffset(1, textOffset, 0) // slot 1: text (required)
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 4.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Self-remove from a MLS group (SelfRemove proposal + commit).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Leave`) for the byte doorway.
    fun marmotLeave(
        correlationId: String,
        groupIdHex: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val gidOffset = fbb.createString(groupIdHex)
        fbb.startTable(1)
        fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 5.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Remove other members from a MLS group (Remove proposal + commit).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Remove`) for the byte doorway.
    fun marmotRemove(
        correlationId: String,
        groupIdHex: String,
        memberNpubs: List<String> = emptyList(),
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val npubsVec = run {
            val offs = IntArray(memberNpubs.size) { i -> fbb.createString(memberNpubs[i]) }
            fbb.startVector(4, offs.size, 4)
            for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])
            fbb.endVector()
        }
        val gidOffset = fbb.createString(groupIdHex)
        fbb.startTable(2)
        fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)
        fbb.addOffset(1, npubsVec, 0) // slot 1: member_npubs
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 6.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Accept a pending MLS Welcome (by gift-wrap event id hex).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `AcceptWelcome`) for the byte doorway.
    fun marmotAcceptWelcome(
        correlationId: String,
        welcomeIdHex: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val widOffset = fbb.createString(welcomeIdHex)
        fbb.startTable(1)
        fbb.addOffset(0, widOffset, 0) // slot 0: welcome_id_hex (required)
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 7.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Decline a pending MLS Welcome.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `DeclineWelcome`) for the byte doorway.
    fun marmotDeclineWelcome(
        correlationId: String,
        welcomeIdHex: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val widOffset = fbb.createString(welcomeIdHex)
        fbb.startTable(1)
        fbb.addOffset(0, widOffset, 0) // slot 0: welcome_id_hex (required)
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 8.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }

    /// Explicitly clear the pending-commit state for a MLS group.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `ClearPending`) for the byte doorway.
    fun marmotClearPending(
        correlationId: String,
        groupIdHex: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val gidOffset = fbb.createString(groupIdHex)
        fbb.startTable(1)
        fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, 9.toByte(), 0) // slot 1: body_type
        fbb.addOffset(2, bodyOffset, 0) // slot 2: body
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NMMA")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.marmot",
            payload = payload,
        )
    }
}
