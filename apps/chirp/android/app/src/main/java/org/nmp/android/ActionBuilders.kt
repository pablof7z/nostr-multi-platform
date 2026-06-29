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
    enum class PublishSignerProvenance(val token: String) {
        APP_MANAGED("app_managed"),
        USER_SELECTED("user_selected"),
        PROTOCOL_PINNED("protocol_pinned"),
        DIAGNOSTIC("diagnostic"),
    }

    sealed class PublishSignerSelection {
        object Active : PublishSignerSelection()
        data class Registered(
            val pubkey: String,
            val provenance: PublishSignerProvenance = PublishSignerProvenance.APP_MANAGED,
        ) : PublishSignerSelection()
    }

    enum class PublishRouteClass(val token: String) {
        MANUAL_OVERRIDE("manual_override"),
        GROUP_HOST_PIN("group_host_pin"),
        VERIFIED_PRIVATE_INBOX("verified_private_inbox"),
        IMPORTED_OR_PRESIGNED("imported_or_presigned"),
        DIAGNOSTIC("diagnostic"),
    }

    sealed class PublishTargetSelection {
        object Auto : PublishTargetSelection()
        data class Explicit(
            val relays: List<String>,
            val routeClass: PublishRouteClass,
        ) : PublishTargetSelection()
    }

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

    /// Publish a NIP-84 kind:9802 highlight annotation.
    /// Builds the `nmp.nip84.publish_highlight` `DispatchEnvelope` bytes for the byte doorway.
    fun publishHighlight(
        correlationId: String,
        content: String,
        context: String?,
        sourceEventId: String?,
        sourceAddress: String?,
        sourceAuthorPubkey: String?,
        alt: String?,
        externalIds: List<String>?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val contentOffset = fbb.createString(content)
        val contextOffset = context?.let { fbb.createString(it) } ?: 0
        val sourceEventIdOffset = sourceEventId?.let { fbb.createString(it) } ?: 0
        val sourceAddressOffset = sourceAddress?.let { fbb.createString(it) } ?: 0
        val sourceAuthorPubkeyOffset = sourceAuthorPubkey?.let { fbb.createString(it) } ?: 0
        val altOffset = alt?.let { fbb.createString(it) } ?: 0
        val externalIdsOffset = run {
            val values = externalIds
            if (values == null || values.isEmpty()) 0 else {
                val offsets = IntArray(values.size) { i -> fbb.createString(values[i]) }
                fbb.startVector(4, offsets.size, 4)
                for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
                fbb.endVector()
            }
        }
        fbb.startTable(8)
        fbb.addInt(0, 2, 0) // slot 0: schema_version
        fbb.addOffset(1, contentOffset, 0) // slot 1: content
        if (contextOffset != 0) fbb.addOffset(2, contextOffset, 0) // slot 2: context
        if (sourceEventIdOffset != 0) fbb.addOffset(3, sourceEventIdOffset, 0) // slot 3: sourceEventId
        if (sourceAddressOffset != 0) fbb.addOffset(4, sourceAddressOffset, 0) // slot 4: sourceAddress
        if (sourceAuthorPubkeyOffset != 0) fbb.addOffset(5, sourceAuthorPubkeyOffset, 0) // slot 5: sourceAuthorPubkey
        if (altOffset != 0) fbb.addOffset(6, altOffset, 0) // slot 6: alt
        if (externalIdsOffset != 0) fbb.addOffset(7, externalIdsOffset, 0) // slot 7: externalIds
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N84H")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip84.publish_highlight",
            payload = payload,
        )
    }

    /// Publish a NIP-22 kind:1111 comment.
    /// Builds the `nmp.nip22.post_comment` `DispatchEnvelope` bytes for the byte doorway.
    fun postComment(
        correlationId: String,
        rootTagName: String,
        rootTagValue: String,
        rootKind: Int,
        parentEventId: String?,
        rootAuthorPubkey: String?,
        parentAuthorPubkey: String?,
        content: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val rootTagNameOffset = fbb.createString(rootTagName)
        val rootTagValueOffset = fbb.createString(rootTagValue)
        val parentEventIdOffset = parentEventId?.let { fbb.createString(it) } ?: 0
        val rootAuthorPubkeyOffset = rootAuthorPubkey?.let { fbb.createString(it) } ?: 0
        val parentAuthorPubkeyOffset = parentAuthorPubkey?.let { fbb.createString(it) } ?: 0
        val contentOffset = fbb.createString(content)
        fbb.startTable(8)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, rootTagNameOffset, 0) // slot 1: rootTagName
        fbb.addOffset(2, rootTagValueOffset, 0) // slot 2: rootTagValue
        fbb.addInt(3, rootKind, 0) // slot 3: rootKind
        if (parentEventIdOffset != 0) fbb.addOffset(4, parentEventIdOffset, 0) // slot 4: parentEventId
        if (rootAuthorPubkeyOffset != 0) fbb.addOffset(5, rootAuthorPubkeyOffset, 0) // slot 5: rootAuthorPubkey
        if (parentAuthorPubkeyOffset != 0) fbb.addOffset(6, parentAuthorPubkeyOffset, 0) // slot 6: parentAuthorPubkey
        fbb.addOffset(7, contentOffset, 0) // slot 7: content
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N22C")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip22.post_comment",
            payload = payload,
        )
    }

    /// Add an item to a NIP-51 kind:30003 bookmark or kind:30004 curation set.
    /// Builds the `nmp.nip51.add_bookmark_set_item` `DispatchEnvelope` bytes for the byte doorway.
    fun addBookmarkSetItem(
        correlationId: String,
        accountPubkey: String,
        setKind: Byte,
        identifier: String,
        itemKind: Byte,
        value: String,
        relay: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        val identifierOffset = fbb.createString(identifier)
        val valueOffset = fbb.createString(value)
        val relayOffset = relay?.let { fbb.createString(it) } ?: 0
        fbb.startTable(3)
        fbb.addByte(0, itemKind, 0) // slot 0: kind
        fbb.addOffset(1, valueOffset, 0) // slot 1: value
        if (relayOffset != 0) fbb.addOffset(2, relayOffset, 0) // slot 2: relay
        val itemRoot = fbb.endTable()
        fbb.startTable(5)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: account_pubkey
        fbb.addByte(2, setKind, 0) // slot 2: set_kind
        fbb.addOffset(3, identifierOffset, 0) // slot 3: identifier
        fbb.addOffset(4, itemRoot, 0) // slot 4: item
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N51S")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.add_bookmark_set_item",
            payload = payload,
        )
    }

    /// Remove an item from a NIP-51 kind:30003 bookmark or kind:30004 curation set.
    /// Builds the `nmp.nip51.remove_bookmark_set_item` `DispatchEnvelope` bytes for the byte doorway.
    fun removeBookmarkSetItem(
        correlationId: String,
        accountPubkey: String,
        setKind: Byte,
        identifier: String,
        itemKind: Byte,
        value: String,
        relay: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        val identifierOffset = fbb.createString(identifier)
        val valueOffset = fbb.createString(value)
        val relayOffset = relay?.let { fbb.createString(it) } ?: 0
        fbb.startTable(3)
        fbb.addByte(0, itemKind, 0) // slot 0: kind
        fbb.addOffset(1, valueOffset, 0) // slot 1: value
        if (relayOffset != 0) fbb.addOffset(2, relayOffset, 0) // slot 2: relay
        val itemRoot = fbb.endTable()
        fbb.startTable(5)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: account_pubkey
        fbb.addByte(2, setKind, 0) // slot 2: set_kind
        fbb.addOffset(3, identifierOffset, 0) // slot 3: identifier
        fbb.addOffset(4, itemRoot, 0) // slot 4: item
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N51S")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.remove_bookmark_set_item",
            payload = payload,
        )
    }

    /// Publish or update a NIP-B0 kind:39701 web bookmark.
    /// Builds the `nmp.nip51.publish_web_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    fun publishWebBookmark(
        correlationId: String,
        accountPubkey: String,
        url: String,
        title: String?,
        description: String?,
        publishedAt: Long?,
        hashtags: List<String>?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val accountPubkeyOffset = fbb.createString(accountPubkey)
        val urlOffset = fbb.createString(url)
        val titleOffset = title?.let { fbb.createString(it) } ?: 0
        val descriptionOffset = description?.let { fbb.createString(it) } ?: 0
        val hashtagsOffset = run {
            val values = hashtags
            if (values == null || values.isEmpty()) 0 else {
                val offsets = IntArray(values.size) { i -> fbb.createString(values[i]) }
                fbb.startVector(4, offsets.size, 4)
                for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
                fbb.endVector()
            }
        }
        fbb.startTable(8)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: accountPubkey
        fbb.addOffset(2, urlOffset, 0) // slot 2: url
        if (titleOffset != 0) fbb.addOffset(3, titleOffset, 0) // slot 3: title
        if (descriptionOffset != 0) fbb.addOffset(4, descriptionOffset, 0) // slot 4: description
        if (publishedAt != null) {
            fbb.addLong(5, publishedAt, 0L) // slot 5: publishedAt
            fbb.addBoolean(6, true, false) // slot 6: hasPublishedAt
        }
        if (hashtagsOffset != 0) fbb.addOffset(7, hashtagsOffset, 0) // slot 7: hashtags
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N51W")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip51.publish_web_bookmark",
            payload = payload,
        )
    }

    /// Upload a file via BUD-02 to one or more Blossom servers.
    /// Builds the `nmp.blossom.upload` `DispatchEnvelope` bytes for the byte doorway.
    fun blossomUpload(
        correlationId: String,
        filePath: String,
        contentType: String?,
        servers: List<String>?,
        signerPubkey: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val filePathOffset = fbb.createString(filePath)
        val contentTypeOffset = contentType?.let { fbb.createString(it) } ?: 0
        val serversOffset = run {
            val values = servers
            if (values == null || values.isEmpty()) 0 else {
                val offsets = IntArray(values.size) { i -> fbb.createString(values[i]) }
                fbb.startVector(4, offsets.size, 4)
                for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
                fbb.endVector()
            }
        }
        val signerPubkeyOffset = signerPubkey?.let { fbb.createString(it) } ?: 0
        fbb.startTable(5)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, filePathOffset, 0) // slot 1: filePath
        if (contentTypeOffset != 0) fbb.addOffset(2, contentTypeOffset, 0) // slot 2: contentType
        if (serversOffset != 0) fbb.addOffset(3, serversOffset, 0) // slot 3: servers
        if (signerPubkeyOffset != 0) fbb.addOffset(4, signerPubkeyOffset, 0) // slot 4: signerPubkey
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "BUPL")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.blossom.upload",
            payload = payload,
        )
    }

    /// Open or close a relay-pinned browse subscription.
    /// Builds the `nmp.browse_relay` `DispatchEnvelope` bytes for the byte doorway.
    fun browseRelay(
        correlationId: String,
        op: Byte,
        relayUrl: String?,
        kinds: List<Int>?,
        lifecycle: Byte,
        interestId: Long,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relayUrlOffset = relayUrl?.let { fbb.createString(it) } ?: 0
        val kindsOffset = run {
            val values = kinds
            if (values == null || values.isEmpty()) 0 else {
                fbb.startVector(4, values.size, 4)
                for (i in values.size - 1 downTo 0) fbb.addInt(values[i])
                fbb.endVector()
            }
        }
        fbb.startTable(6)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, op, 0) // slot 1: op
        if (relayUrlOffset != 0) fbb.addOffset(2, relayUrlOffset, 0) // slot 2: relayUrl
        if (kindsOffset != 0) fbb.addOffset(3, kindsOffset, 0) // slot 3: kinds
        fbb.addByte(4, lifecycle, 0) // slot 4: lifecycle
        fbb.addLong(5, interestId, 0L) // slot 5: interestId
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NBRW")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.browse_relay",
            payload = payload,
        )
    }

    /// Claim or release a NIP-23 topic-articles subscription.
    /// Builds the `nmp.app.topic_articles` `DispatchEnvelope` bytes for the byte doorway.
    fun topicArticles(
        correlationId: String,
        op: Byte,
        topic: String,
        consumerId: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val topicOffset = fbb.createString(topic)
        val consumerIdOffset = fbb.createString(consumerId)
        fbb.startTable(4)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addByte(1, op, 0) // slot 1: op
        fbb.addOffset(2, topicOffset, 0) // slot 2: topic
        fbb.addOffset(3, consumerIdOffset, 0) // slot 3: consumerId
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "NTPC")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.app.topic_articles",
            payload = payload,
        )
    }

    /// Discover NIP-29 groups hosted on a relay.
    /// Builds the `nmp.nip29.discover` `DispatchEnvelope` bytes for the byte doorway.
    fun discoverGroups(
        correlationId: String,
        relayUrl: String,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val relayUrlOffset = fbb.createString(relayUrl)
        fbb.startTable(2)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, relayUrlOffset, 0) // slot 1: relayUrl
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N29D")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip29.discover",
            payload = payload,
        )
    }

    /// Publish an event to a NIP-29 group (any kind).
    /// Builds the `nmp.nip29.publish_group_event` `DispatchEnvelope` bytes for the byte doorway.
    fun publishGroupEvent(
        correlationId: String,
        group: Pair<String, String>,
        kind: Int,
        content: String?,
        tags: List<List<String>>?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val groupHostRelayUrlOffset = fbb.createString(group.first)
        val groupLocalIdOffset = fbb.createString(group.second)
        fbb.startTable(2)
        fbb.addOffset(0, groupHostRelayUrlOffset, 0) // GroupRef slot 0: host_relay_url
        fbb.addOffset(1, groupLocalIdOffset, 0) // GroupRef slot 1: local_id
        val groupOffset = fbb.endTable()
        val contentOffset = content?.let { fbb.createString(it) } ?: 0
        val tagsOffset = run {
            val tagRows = tags
            if (tagRows == null || tagRows.isEmpty()) 0 else {
                val tagOffsets = IntArray(tagRows.size) { i ->
                    val row = tagRows[i]
                    val valOffsets = IntArray(row.size) { j -> fbb.createString(row[j]) }
                    fbb.startVector(4, valOffsets.size, 4)
                    for (k in valOffsets.size - 1 downTo 0) fbb.addOffset(valOffsets[k])
                    val valsVec = fbb.endVector()
                    fbb.startTable(1)
                    fbb.addOffset(0, valsVec, 0) // StringTag slot 0: values
                    fbb.endTable()
                }
                fbb.startVector(4, tagOffsets.size, 4)
                for (i in tagOffsets.size - 1 downTo 0) fbb.addOffset(tagOffsets[i])
                fbb.endVector()
            }
        }
        fbb.startTable(5)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, groupOffset, 0) // slot 1: group
        fbb.addInt(2, kind, 0) // slot 2: kind
        if (contentOffset != 0) fbb.addOffset(3, contentOffset, 0) // slot 3: content
        if (tagsOffset != 0) fbb.addOffset(4, tagsOffset, 0) // slot 4: tags
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N29G")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip29.publish_group_event",
            payload = payload,
        )
    }

    /// Request membership in a NIP-29 group.
    /// Builds the `nmp.nip29.join` `DispatchEnvelope` bytes for the byte doorway.
    fun joinGroup(
        correlationId: String,
        group: Pair<String, String>,
        inviteCode: String?,
        reason: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val groupHostRelayUrlOffset = fbb.createString(group.first)
        val groupLocalIdOffset = fbb.createString(group.second)
        fbb.startTable(2)
        fbb.addOffset(0, groupHostRelayUrlOffset, 0) // GroupRef slot 0: host_relay_url
        fbb.addOffset(1, groupLocalIdOffset, 0) // GroupRef slot 1: local_id
        val groupOffset = fbb.endTable()
        val inviteCodeOffset = inviteCode?.let { fbb.createString(it) } ?: 0
        val reasonOffset = reason?.let { fbb.createString(it) } ?: 0
        fbb.startTable(4)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, groupOffset, 0) // slot 1: group
        if (inviteCodeOffset != 0) fbb.addOffset(2, inviteCodeOffset, 0) // slot 2: inviteCode
        if (reasonOffset != 0) fbb.addOffset(3, reasonOffset, 0) // slot 3: reason
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N29J")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip29.join",
            payload = payload,
        )
    }

    /// Create a new public NIP-29 group.
    /// Builds the `nmp.nip29.create_public_group` `DispatchEnvelope` bytes for the byte doorway.
    fun createPublicGroup(
        correlationId: String,
        group: Pair<String, String>,
        name: String,
        about: String?,
        picture: String?,
        visibility: Byte,
        access: Byte,
        parent: String?,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val groupHostRelayUrlOffset = fbb.createString(group.first)
        val groupLocalIdOffset = fbb.createString(group.second)
        fbb.startTable(2)
        fbb.addOffset(0, groupHostRelayUrlOffset, 0) // GroupRef slot 0: host_relay_url
        fbb.addOffset(1, groupLocalIdOffset, 0) // GroupRef slot 1: local_id
        val groupOffset = fbb.endTable()
        val nameOffset = fbb.createString(name)
        val aboutOffset = about?.let { fbb.createString(it) } ?: 0
        val pictureOffset = picture?.let { fbb.createString(it) } ?: 0
        val parentOffset = parent?.let { fbb.createString(it) } ?: 0
        fbb.startTable(8)
        fbb.addInt(0, 1, 0) // slot 0: schema_version
        fbb.addOffset(1, groupOffset, 0) // slot 1: group
        fbb.addOffset(2, nameOffset, 0) // slot 2: name
        if (aboutOffset != 0) fbb.addOffset(3, aboutOffset, 0) // slot 3: about
        if (pictureOffset != 0) fbb.addOffset(4, pictureOffset, 0) // slot 4: picture
        fbb.addByte(5, visibility, 0) // slot 5: visibility
        fbb.addByte(6, access, 0) // slot 6: access
        if (parentOffset != 0) fbb.addOffset(7, parentOffset, 0) // slot 7: parent
        val payloadRoot = fbb.endTable()
        fbb.finish(payloadRoot, "N29P")
        val payload = fbb.sizedByteArray()
        return encodeDispatchEnvelope(
            correlationId = correlationId,
            actionNamespace = "nmp.nip29.create_public_group",
            payload = payload,
        )
    }

    /// Low-level arbitrary-kind publish escape; starter apps should prefer protocol/product builders such as publishReply or publishProfile.
    /// Requires typed signer selection and route provenance for explicit targets; not the starter happy path.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishRaw`) for the byte doorway.
    fun publishRaw(
        correlationId: String,
        kind: Int,
        tags: List<List<String>>,
        content: String,
        target: PublishTargetSelection = PublishTargetSelection.Auto,
        signer: PublishSignerSelection = PublishSignerSelection.Active,
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
        val signerOffset = when (signer) {
            PublishSignerSelection.Active -> 0
            is PublishSignerSelection.Registered -> {
                val signerPubkeyOffset = fbb.createString(signer.pubkey)
                val signerProvenanceOffset = fbb.createString(signer.provenance.token)
                fbb.startTable(3)
                fbb.addByte(0, 1.toByte(), 0) // slot 0: mode (Registered)
                fbb.addOffset(1, signerPubkeyOffset, 0) // slot 1: pubkey
                fbb.addOffset(2, signerProvenanceOffset, 0) // slot 2: provenance
                fbb.endTable()
            }
        }
        val targetRelays = when (target) {
            PublishTargetSelection.Auto -> emptyList()
            is PublishTargetSelection.Explicit -> target.relays
        }
        val explicit = target is PublishTargetSelection.Explicit
        val targetRelaysVec = run {
            val offsets = IntArray(targetRelays.size) { i -> fbb.createString(targetRelays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        val routeClassOffset = when (target) {
            PublishTargetSelection.Auto -> 0
            is PublishTargetSelection.Explicit -> fbb.createString(target.routeClass.token)
        }
        fbb.startTable(3)
        fbb.addBoolean(0, explicit, false) // slot 0: explicit
        fbb.addOffset(1, targetRelaysVec, 0) // slot 1: relays
        if (routeClassOffset != 0) fbb.addOffset(2, routeClassOffset, 0) // slot 2: route_class
        val targetOffset = fbb.endTable()
        fbb.startTable(5)
        fbb.addInt(0, kind, 0) // slot 0: kind
        fbb.addOffset(1, tagsVec, 0) // slot 1: tags
        fbb.addOffset(2, contentOffset, 0) // slot 2: content
        fbb.addOffset(3, targetOffset, 0) // slot 3: target
        if (signerOffset != 0) fbb.addOffset(4, signerOffset, 0) // slot 4: signer
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 4, 0) // slot 0: schema_version
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

    /// Sign-and-publish a kind:1 reply; Rust derives NIP-10 tags from the stored parent event.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishReply`) for the byte doorway.
    fun publishReply(
        correlationId: String,
        content: String,
        replyToEventId: String,
        target: PublishTargetSelection = PublishTargetSelection.Auto,
        signer: PublishSignerSelection = PublishSignerSelection.Active,
    ): ByteArray {
        val fbb = FlatBufferBuilder()
        val contentOffset = fbb.createString(content)
        val replyToEventIdOffset = fbb.createString(replyToEventId)
        val signerOffset = when (signer) {
            PublishSignerSelection.Active -> 0
            is PublishSignerSelection.Registered -> {
                val signerPubkeyOffset = fbb.createString(signer.pubkey)
                val signerProvenanceOffset = fbb.createString(signer.provenance.token)
                fbb.startTable(3)
                fbb.addByte(0, 1.toByte(), 0) // slot 0: mode (Registered)
                fbb.addOffset(1, signerPubkeyOffset, 0) // slot 1: pubkey
                fbb.addOffset(2, signerProvenanceOffset, 0) // slot 2: provenance
                fbb.endTable()
            }
        }
        val targetRelays = when (target) {
            PublishTargetSelection.Auto -> emptyList()
            is PublishTargetSelection.Explicit -> target.relays
        }
        val explicit = target is PublishTargetSelection.Explicit
        val targetRelaysVec = run {
            val offsets = IntArray(targetRelays.size) { i -> fbb.createString(targetRelays[i]) }
            fbb.startVector(4, offsets.size, 4)
            for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])
            fbb.endVector()
        }
        val routeClassOffset = when (target) {
            PublishTargetSelection.Auto -> 0
            is PublishTargetSelection.Explicit -> fbb.createString(target.routeClass.token)
        }
        fbb.startTable(3)
        fbb.addBoolean(0, explicit, false) // slot 0: explicit
        fbb.addOffset(1, targetRelaysVec, 0) // slot 1: relays
        if (routeClassOffset != 0) fbb.addOffset(2, routeClassOffset, 0) // slot 2: route_class
        val targetOffset = fbb.endTable()
        fbb.startTable(4)
        fbb.addOffset(0, contentOffset, 0) // slot 0: content
        fbb.addOffset(1, replyToEventIdOffset, 0) // slot 1: reply_to_event_id
        fbb.addOffset(2, targetOffset, 0) // slot 2: target
        if (signerOffset != 0) fbb.addOffset(3, signerOffset, 0) // slot 3: signer
        val bodyOffset = fbb.endTable()
        fbb.startTable(3)
        fbb.addInt(0, 4, 0) // slot 0: schema_version
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
        fbb.addInt(0, 4, 0) // slot 0: schema_version
        fbb.addByte(1, 1.toByte(), 0) // slot 1: body_type
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
