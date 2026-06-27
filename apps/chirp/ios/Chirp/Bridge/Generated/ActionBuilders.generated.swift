// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform swift \
//       --out apps/chirp/ios/Chirp/Bridge/Generated/ActionBuilders.generated.swift
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated Swift differs from a fresh run.
//
// ADR-0064 §3 — typed write builders. Each function below encodes the per-crate
// FlatBuffers payload for one open-registry `action_namespace` and stamps it,
// the namespace, and the envelope schema_version into a `DispatchEnvelope`,
// returning the finished bytes for the native byte doorway
// `nmp_app_dispatch_action_bytes` (#1752). App code NEVER spells a namespace
// string or hand-assembles FlatBuffers — that lives only here, in generated
// code. The host supplies the `correlation_id` (the operation identity end to
// end, ADR-0064 §4) and owns the FFI call.
// ─────────────────────────────────────────────────────────────────────────────

import FlatBuffers
import Foundation

public enum GeneratedActionBuilders {
    /// The single recognised envelope schema version — mirrors
    /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.
    public static let dispatchEnvelopeSchemaVersion: UInt32 = 1

    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a
    /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).
    /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.
    private static func encodeDispatchEnvelope(
        correlationId: String,
        actionNamespace: String,
        payload: [UInt8]
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let correlationOffset = fbb.create(string: correlationId)
        let namespaceOffset = fbb.create(string: actionNamespace)
        let payloadOffset = fbb.createVector(payload)
        let start = fbb.startTable(with: 4)
        fbb.add(offset: correlationOffset, at: 4)   // slot 0: correlation_id
        fbb.add(offset: namespaceOffset, at: 6)     // slot 1: action_namespace
        fbb.add(element: dispatchEnvelopeSchemaVersion, def: UInt32(0), at: 8) // slot 2: schema_version
        fbb.add(offset: payloadOffset, at: 10)      // slot 3: payload
        let root = Offset(offset: fbb.endTable(at: start))
        fbb.finish(offset: root, fileId: "NMPD")
        return fbb.sizedByteArray
    }

    /// Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
    /// mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
    /// Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
    /// so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
    /// Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive.
    private static func relayMarkerByte(_ role: String) -> UInt8 {
        var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false
        var invalid = false
        for part in role.split(separator: ",").map({ $0.trimmingCharacters(in: .whitespaces).lowercased() }) {
            switch part {
            case "": break
            case "both": hasBoth = true
            case "read": hasRead = true
            case "write": hasWrite = true
            case "indexer": hasIndexer = true
            default: invalid = true
            }
        }
        if invalid { return 255 }
        if hasBoth || (hasRead && hasWrite) { return 0 }
        if hasRead { return 1 }
        if hasWrite { return 2 }
        if hasIndexer { return 3 }
        return 255
    }

    /// Publish a NIP-25 reaction to a target event.
    /// Builds the `nmp.nip25.react` `DispatchEnvelope` bytes for the byte doorway.
    public static func react(
        correlationId: String,
        targetEventId: String,
        reaction: String,
        targetAuthorPubkey: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let targetEventIdOffset = fbb.create(string: targetEventId)
        let reactionOffset = fbb.create(string: reaction)
        let targetAuthorPubkeyOffset: Offset = targetAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: targetEventIdOffset, at: 6) // slot 1: targetEventId
        fbb.add(offset: reactionOffset, at: 8) // slot 2: reaction
        if targetAuthorPubkeyOffset.o != 0 { fbb.add(offset: targetAuthorPubkeyOffset, at: 10) } // slot 3: targetAuthorPubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N25R")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip25.react",
            payload: payload
        )
    }

    /// Retract a previously-published NIP-25 reaction.
    /// Builds the `nmp.nip25.unreact` `DispatchEnvelope` bytes for the byte doorway.
    public static func unreact(
        correlationId: String,
        reactionEventId: String,
        reason: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let reactionEventIdOffset = fbb.create(string: reactionEventId)
        let reasonOffset = fbb.create(string: reason)
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: reactionEventIdOffset, at: 6) // slot 1: reactionEventId
        fbb.add(offset: reasonOffset, at: 8) // slot 2: reason
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N25U")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip25.unreact",
            payload: payload
        )
    }

    /// Publish a NIP-18 repost wrapper for a target event.
    /// Builds the `nmp.nip18.repost` `DispatchEnvelope` bytes for the byte doorway.
    public static func repost(
        correlationId: String,
        targetEventId: String,
        targetKind: UInt32,
        targetAuthorPubkey: String?,
        relayHint: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let targetEventIdOffset = fbb.create(string: targetEventId)
        let targetAuthorPubkeyOffset: Offset = targetAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let relayHintOffset: Offset = relayHint.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 5)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: targetEventIdOffset, at: 6) // slot 1: targetEventId
        fbb.add(element: UInt32(targetKind), def: UInt32(0), at: 8) // slot 2: targetKind
        if targetAuthorPubkeyOffset.o != 0 { fbb.add(offset: targetAuthorPubkeyOffset, at: 10) } // slot 3: targetAuthorPubkey
        if relayHintOffset.o != 0 { fbb.add(offset: relayHintOffset, at: 12) } // slot 4: relayHint
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N18R")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip18.repost",
            payload: payload
        )
    }

    /// Publish a NIP-18 quote repost note for a target event.
    /// Builds the `nmp.nip18.quote_repost` `DispatchEnvelope` bytes for the byte doorway.
    public static func quoteRepost(
        correlationId: String,
        targetEventId: String,
        targetKind: UInt32,
        targetAuthorPubkey: String?,
        relayHint: String?,
        content: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let targetEventIdOffset = fbb.create(string: targetEventId)
        let targetAuthorPubkeyOffset: Offset = targetAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let relayHintOffset: Offset = relayHint.map { fbb.create(string: $0) } ?? Offset()
        let contentOffset = fbb.create(string: content)
        let payloadStart = fbb.startTable(with: 6)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: targetEventIdOffset, at: 6) // slot 1: targetEventId
        fbb.add(element: UInt32(targetKind), def: UInt32(0), at: 8) // slot 2: targetKind
        if targetAuthorPubkeyOffset.o != 0 { fbb.add(offset: targetAuthorPubkeyOffset, at: 10) } // slot 3: targetAuthorPubkey
        if relayHintOffset.o != 0 { fbb.add(offset: relayHintOffset, at: 12) } // slot 4: relayHint
        fbb.add(offset: contentOffset, at: 14) // slot 5: content
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N18Q")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip18.quote_repost",
            payload: payload
        )
    }

    /// Follow a single pubkey (NIP-02 contact-list add).
    /// Builds the `nmp.follow` `DispatchEnvelope` bytes for the byte doorway.
    public static func follow(
        correlationId: String,
        pubkey: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let pubkeyOffset = fbb.create(string: pubkey)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: pubkeyOffset, at: 6) // slot 1: pubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NF2A")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.follow",
            payload: payload
        )
    }

    /// Unfollow a single pubkey (NIP-02 contact-list remove).
    /// Builds the `nmp.unfollow` `DispatchEnvelope` bytes for the byte doorway.
    public static func unfollow(
        correlationId: String,
        pubkey: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let pubkeyOffset = fbb.create(string: pubkey)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: pubkeyOffset, at: 6) // slot 1: pubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NF2A")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.unfollow",
            payload: payload
        )
    }

    /// Follow many pubkeys in one race-free read-modify-write cycle (NIP-02).
    /// Builds the `nmp.follow_many` `DispatchEnvelope` bytes for the byte doorway.
    public static func followMany(
        correlationId: String,
        pubkeys: [String]?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let pubkeysOffset: Offset = {
            guard let values = pubkeys, !values.isEmpty else { return Offset() }
            let offsets = values.map { fbb.create(string: $0) }
            return fbb.createVector(ofOffsets: offsets)
        }()
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        if pubkeysOffset.o != 0 { fbb.add(offset: pubkeysOffset, at: 6) } // slot 1: pubkeys
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NFMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.follow_many",
            payload: payload
        )
    }

    /// Add one item to the active account's NIP-51 bookmark list.
    /// Builds the `nmp.nip51.add_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    public static func addBookmark(
        correlationId: String,
        accountPubkey: String,
        itemKind: UInt8,
        value: String,
        relay: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let valueOffset = fbb.create(string: value)
        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()
        let itemStart = fbb.startTable(with: 3)
        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind
        fbb.add(offset: valueOffset, at: 6) // slot 1: value
        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay
        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey
        fbb.add(offset: itemRoot, at: 8) // slot 2: item
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N51B")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.add_bookmark",
            payload: payload
        )
    }

    /// Remove one item from the active account's NIP-51 bookmark list.
    /// Builds the `nmp.nip51.remove_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    public static func removeBookmark(
        correlationId: String,
        accountPubkey: String,
        itemKind: UInt8,
        value: String,
        relay: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let valueOffset = fbb.create(string: value)
        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()
        let itemStart = fbb.startTable(with: 3)
        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind
        fbb.add(offset: valueOffset, at: 6) // slot 1: value
        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay
        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey
        fbb.add(offset: itemRoot, at: 8) // slot 2: item
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N51B")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.remove_bookmark",
            payload: payload
        )
    }

    /// Add a relay URL to the NIP-51 blocked-relay list.
    /// Builds the `nmp.nip51.block_relay` `DispatchEnvelope` bytes for the byte doorway.
    public static func blockRelay(
        correlationId: String,
        url: String,
        accountPubkey: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let urlOffset = fbb.create(string: url)
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: urlOffset, at: 6) // slot 1: url
        fbb.add(offset: accountPubkeyOffset, at: 8) // slot 2: accountPubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NBLK")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.block_relay",
            payload: payload
        )
    }

    /// Remove a relay URL from the NIP-51 blocked-relay list.
    /// Builds the `nmp.nip51.unblock_relay` `DispatchEnvelope` bytes for the byte doorway.
    public static func unblockRelay(
        correlationId: String,
        url: String,
        accountPubkey: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let urlOffset = fbb.create(string: url)
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: urlOffset, at: 6) // slot 1: url
        fbb.add(offset: accountPubkeyOffset, at: 8) // slot 2: accountPubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NUBL")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.unblock_relay",
            payload: payload
        )
    }

    /// Publish a NIP-17 DM relay list (kind:10050).
    /// Builds the `nmp.nip17.publish_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishDmRelayList(
        correlationId: String,
        relays: [String]
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let relaysOffsets = relays.map { fbb.create(string: $0) }
        let relaysOffset = fbb.createVector(ofOffsets: relaysOffsets)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: relaysOffset, at: 6) // slot 1: relays
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N17R")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip17.publish_relay_list",
            payload: payload
        )
    }

    /// Hydrate a DM peer's NIP-17 relay list (kind:10050).
    /// Builds the `nmp.nip17.hydrate_peer_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    public static func hydrateDmPeerRelayList(
        correlationId: String,
        peerPubkey: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let peerPubkeyOffset = fbb.create(string: peerPubkey)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: peerPubkeyOffset, at: 6) // slot 1: peerPubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N17H")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip17.hydrate_peer_relay_list",
            payload: payload
        )
    }

    /// Publish a NIP-65 relay-list metadata event (kind:10002).
    /// Builds the `nmp.nip65.publish_relay_list` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishRelayList(
        correlationId: String,
        relays: [(url: String, role: String)]
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        var relaysEntryOffsets: [Offset] = []
        for r in relays {
            let urlOff = fbb.create(string: r.url)
            let entryStart = fbb.startTable(with: 2)
            fbb.add(offset: urlOff, at: 4) // RelayListEntry slot 0: url
            fbb.add(element: Self.relayMarkerByte(r.role), def: UInt8(0), at: 6) // RelayListEntry slot 1: marker
            relaysEntryOffsets.append(Offset(offset: fbb.endTable(at: entryStart)))
        }
        let relaysOffset = fbb.createVector(ofOffsets: relaysEntryOffsets)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: relaysOffset, at: 6) // slot 1: relays
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N65P")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip65.publish_relay_list",
            payload: payload
        )
    }

    /// Connect a NIP-47 Nostr Wallet Connect URI.
    /// Builds the `nmp.wallet.connect` `DispatchEnvelope` bytes for the byte doorway.
    public static func walletConnect(
        correlationId: String,
        uri: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let uriOffset = fbb.create(string: uri)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: uriOffset, at: 6) // slot 1: uri
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N47C")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.wallet.connect",
            payload: payload
        )
    }

    /// Disconnect the current NIP-47 wallet (no payload data beyond schema_version).
    /// Builds the `nmp.wallet.disconnect` `DispatchEnvelope` bytes for the byte doorway.
    public static func walletDisconnect(
        correlationId: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let payloadStart = fbb.startTable(with: 1)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N47D")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.wallet.disconnect",
            payload: payload
        )
    }

    /// Pay a Lightning invoice via the NIP-47 wallet.
    /// Builds the `nmp.wallet.pay_invoice` `DispatchEnvelope` bytes for the byte doorway.
    public static func walletPayInvoice(
        correlationId: String,
        bolt11: String,
        amountMsats: UInt64?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let bolt11Offset = fbb.create(string: bolt11)
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: bolt11Offset, at: 6) // slot 1: bolt11
        if let amountMsatsVal = amountMsats {
            fbb.add(element: amountMsatsVal, def: UInt64(0), at: 8) // slot 2: amountMsats
            fbb.add(element: true, def: false, at: 10) // slot 3: hasAmountMsats
        }
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N47P")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.wallet.pay_invoice",
            payload: payload
        )
    }

    /// Send a NIP-17 gift-wrapped direct message to a recipient.
    /// Builds the `nmp.nip17.send` `DispatchEnvelope` bytes for the byte doorway.
    public static func sendDm(
        correlationId: String,
        recipientPubkey: String,
        content: String,
        replyTo: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let recipientPubkeyOffset = fbb.create(string: recipientPubkey)
        let contentOffset = fbb.create(string: content)
        let replyToOffset: Offset = replyTo.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: recipientPubkeyOffset, at: 6) // slot 1: recipientPubkey
        fbb.add(offset: contentOffset, at: 8) // slot 2: content
        if replyToOffset.o != 0 { fbb.add(offset: replyToOffset, at: 10) } // slot 3: replyTo
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N17S")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip17.send",
            payload: payload
        )
    }

    /// Publish a NIP-57 zap request for a recipient (optionally a target event).
    /// Builds the `nmp.nip57.zap` `DispatchEnvelope` bytes for the byte doorway.
    public static func zap(
        correlationId: String,
        recipientPubkey: String,
        amountMsats: UInt64,
        lnurl: String?,
        relays: [String],
        targetEventId: String?,
        comment: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let recipientPubkeyOffset = fbb.create(string: recipientPubkey)
        let lnurlOffset: Offset = lnurl.map { fbb.create(string: $0) } ?? Offset()
        let relaysOffsets = relays.map { fbb.create(string: $0) }
        let relaysOffset = fbb.createVector(ofOffsets: relaysOffsets)
        let targetEventIdOffset: Offset = targetEventId.map { fbb.create(string: $0) } ?? Offset()
        let commentOffset: Offset = comment.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 7)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: recipientPubkeyOffset, at: 6) // slot 1: recipientPubkey
        fbb.add(element: amountMsats, def: UInt64(0), at: 8) // slot 2: amountMsats
        if lnurlOffset.o != 0 { fbb.add(offset: lnurlOffset, at: 10) } // slot 3: lnurl
        fbb.add(offset: relaysOffset, at: 12) // slot 4: relays
        if targetEventIdOffset.o != 0 { fbb.add(offset: targetEventIdOffset, at: 14) } // slot 5: targetEventId
        if commentOffset.o != 0 { fbb.add(offset: commentOffset, at: 16) } // slot 6: comment
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N57Z")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip57.zap",
            payload: payload
        )
    }

    /// Sign-and-publish an arbitrary event kind (generic publish path; NIP-65 outbox or explicit relays).
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishRaw`) for the byte doorway.
    public static func publishRaw(
        correlationId: String,
        kind: UInt32,
        tags: [[String]],
        content: String,
        relays: [String]? = nil,
        signerPubkey: String? = nil
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let tagRowOffsets: [Offset] = tags.map { row in
            let valueOffsets = row.map { fbb.create(string: $0) }
            let valuesVec = fbb.createVector(ofOffsets: valueOffsets)
            let start = fbb.startTable(with: 1)
            fbb.add(offset: valuesVec, at: 4) // slot 0: values
            return Offset(offset: fbb.endTable(at: start))
        }
        let tagsVec = fbb.createVector(ofOffsets: tagRowOffsets)
        let contentOffset = fbb.create(string: content)
        let signerPubkeyOffset: Offset = signerPubkey.map { fbb.create(string: $0) } ?? Offset()
        let targetOffset: Offset = {
            let explicit = (relays?.isEmpty == false)
            let relayOffsets = (relays ?? []).map { fbb.create(string: $0) }
            let relaysVec = fbb.createVector(ofOffsets: relayOffsets)
            let start = fbb.startTable(with: 2)
            fbb.add(element: explicit, def: false, at: 4) // slot 0: explicit
            fbb.add(offset: relaysVec, at: 6) // slot 1: relays
            return Offset(offset: fbb.endTable(at: start))
        }()
        let rawStart = fbb.startTable(with: 5)
        fbb.add(element: kind, def: UInt32(0), at: 4) // slot 0: kind
        fbb.add(offset: tagsVec, at: 6) // slot 1: tags
        fbb.add(offset: contentOffset, at: 8) // slot 2: content
        fbb.add(offset: targetOffset, at: 10) // slot 3: target
        if signerPubkeyOffset.o != 0 { fbb.add(offset: signerPubkeyOffset, at: 12) } // slot 4: signer_pubkey
        let bodyOffset = Offset(offset: fbb.endTable(at: rawStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(3), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NPUB")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.publish",
            payload: payload
        )
    }

    /// Sign-and-publish a kind:1 reply; Rust derives NIP-10 tags from the stored parent event.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishReply`) for the byte doorway.
    public static func publishReply(
        correlationId: String,
        content: String,
        replyToEventId: String,
        relays: [String]? = nil,
        signerPubkey: String? = nil
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let contentOffset = fbb.create(string: content)
        let replyToEventIdOffset = fbb.create(string: replyToEventId)
        let signerPubkeyOffset: Offset = signerPubkey.map { fbb.create(string: $0) } ?? Offset()
        let targetOffset: Offset = {
            let explicit = (relays?.isEmpty == false)
            let relayOffsets = (relays ?? []).map { fbb.create(string: $0) }
            let relaysVec = fbb.createVector(ofOffsets: relayOffsets)
            let start = fbb.startTable(with: 2)
            fbb.add(element: explicit, def: false, at: 4) // slot 0: explicit
            fbb.add(offset: relaysVec, at: 6) // slot 1: relays
            return Offset(offset: fbb.endTable(at: start))
        }()
        let replyStart = fbb.startTable(with: 4)
        fbb.add(offset: contentOffset, at: 4) // slot 0: content
        fbb.add(offset: replyToEventIdOffset, at: 6) // slot 1: reply_to_event_id
        fbb.add(offset: targetOffset, at: 8) // slot 2: target
        if signerPubkeyOffset.o != 0 { fbb.add(offset: signerPubkeyOffset, at: 10) } // slot 3: signer_pubkey
        let bodyOffset = Offset(offset: fbb.endTable(at: replyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(4), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NPUB")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.publish",
            payload: payload
        )
    }

    /// Sign-and-publish a kind:0 profile metadata event for the active account.
    /// Builds the `nmp.publish` `DispatchEnvelope` bytes (body `PublishProfile`) for the byte doorway.
    public static func publishProfile(
        correlationId: String,
        fields: [(String, String)]
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let profileFieldOffsets: [Offset] = fields.map { (key, value) in
            let keyOffset = fbb.create(string: key)
            let valueOffset = fbb.create(string: value)
            let start = fbb.startTable(with: 2)
            fbb.add(offset: keyOffset, at: 4) // slot 0: key
            fbb.add(offset: valueOffset, at: 6) // slot 1: value
            return Offset(offset: fbb.endTable(at: start))
        }
        let fieldsVec = fbb.createVector(ofOffsets: profileFieldOffsets)
        let profileStart = fbb.startTable(with: 1)
        fbb.add(offset: fieldsVec, at: 4) // slot 0: fields
        let bodyOffset = Offset(offset: fbb.endTable(at: profileStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(2), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NPUB")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.publish",
            payload: payload
        )
    }

    /// Publish (or rotate) the local MLS key-package (kind:30443) to relays.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `PublishKeyPackage`) for the byte doorway.
    public static func marmotPublishKeyPackage(
        correlationId: String,
        relays: [String] = []
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let relayOffsets = relays.map { fbb.create(string: $0) }
        let relaysVec = fbb.createVector(ofOffsets: relayOffsets)
        let bodyStart = fbb.startTable(with: 1)
        fbb.add(offset: relaysVec, at: 4) // slot 0: relays
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(1), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Create a new MLS group and optionally invite peers.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `CreateGroup`) for the byte doorway.
    public static func marmotCreateGroup(
        correlationId: String,
        name: String,
        description: String = "",
        inviteeText: String? = nil,
        inviteeNpubs: [String]? = nil,
        signedKeyPackageEventsJson: [String] = [],
        relays: [String] = []
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        // Build offsets for nested objects FIRST (FlatBuffers bottom-up).
        // relays + signed_key_package_events_json are NON-OPTIONAL [string]:
        // ALWAYS present (even when empty) to match the Rust encoder (golden
        // byte parity — #2169 / nip02 convention).
        let relayOffsets = relays.map { fbb.create(string: $0) }
        let relaysVec = fbb.createVector(ofOffsets: relayOffsets)
        let jsonOffsets = signedKeyPackageEventsJson.map { fbb.create(string: $0) }
        let jsonVec = fbb.createVector(ofOffsets: jsonOffsets)
        // inviteeNpubs: nil → absent (None); non-nil → present vector (even if empty)
        let npubsVec: Offset? = inviteeNpubs.map { npubs in
            let offs = npubs.map { fbb.create(string: $0) }
            return Offset(offset: fbb.createVector(ofOffsets: offs).o)
        }
        let inviteeTextOffset: Offset? = inviteeText.map { fbb.create(string: $0) }
        let descOffset: Offset? = description.isEmpty ? nil : Optional(fbb.create(string: description))
        let nameOffset = fbb.create(string: name)
        let bodyStart = fbb.startTable(with: 6)
        fbb.add(offset: nameOffset, at: 4) // slot 0: name (required)
        if let descOffset { fbb.add(offset: descOffset, at: 6) } // slot 1: description
        if let inviteeTextOffset { fbb.add(offset: inviteeTextOffset, at: 8) } // slot 2: invitee_text
        if let npubsVec { fbb.add(offset: npubsVec, at: 10) } // slot 3: invitee_npubs
        fbb.add(offset: jsonVec, at: 12) // slot 4: signed_key_package_events_json
        fbb.add(offset: relaysVec, at: 14) // slot 5: relays
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(2), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Invite one or more peers to an existing MLS group.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Invite`) for the byte doorway.
    public static func marmotInvite(
        correlationId: String,
        groupIdHex: String,
        inviteeText: String? = nil,
        inviteeNpubs: [String]? = nil,
        signedKeyPackageEventsJson: [String] = []
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        // signed_key_package_events_json is NON-OPTIONAL [string]: ALWAYS present
        // (even when empty) to match the Rust encoder (golden byte parity — #2169).
        let jsonOffsets = signedKeyPackageEventsJson.map { fbb.create(string: $0) }
        let jsonVec = fbb.createVector(ofOffsets: jsonOffsets)
        let npubsVec: Offset? = inviteeNpubs.map { npubs in
            let offs = npubs.map { fbb.create(string: $0) }
            return Offset(offset: fbb.createVector(ofOffsets: offs).o)
        }
        let inviteeTextOffset: Offset? = inviteeText.map { fbb.create(string: $0) }
        let gidOffset = fbb.create(string: groupIdHex)
        let bodyStart = fbb.startTable(with: 4)
        fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)
        if let inviteeTextOffset { fbb.add(offset: inviteeTextOffset, at: 6) } // slot 1: invitee_text
        if let npubsVec { fbb.add(offset: npubsVec, at: 8) } // slot 2: invitee_npubs
        fbb.add(offset: jsonVec, at: 10) // slot 3: signed_key_package_events_json
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(3), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Send a kind:14 NIP-44 MLS group message.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Send`) for the byte doorway.
    public static func marmotSend(
        correlationId: String,
        groupIdHex: String,
        text: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let textOffset = fbb.create(string: text)
        let gidOffset = fbb.create(string: groupIdHex)
        let bodyStart = fbb.startTable(with: 2)
        fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)
        fbb.add(offset: textOffset, at: 6) // slot 1: text (required)
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(4), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Self-remove from a MLS group (SelfRemove proposal + commit).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Leave`) for the byte doorway.
    public static func marmotLeave(
        correlationId: String,
        groupIdHex: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let gidOffset = fbb.create(string: groupIdHex)
        let bodyStart = fbb.startTable(with: 1)
        fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(5), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Remove other members from a MLS group (Remove proposal + commit).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `Remove`) for the byte doorway.
    public static func marmotRemove(
        correlationId: String,
        groupIdHex: String,
        memberNpubs: [String] = []
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let npubOffsets = memberNpubs.map { fbb.create(string: $0) }
        let npubsVec = fbb.createVector(ofOffsets: npubOffsets)
        let gidOffset = fbb.create(string: groupIdHex)
        let bodyStart = fbb.startTable(with: 2)
        fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)
        fbb.add(offset: npubsVec, at: 6) // slot 1: member_npubs
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(6), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Accept a pending MLS Welcome (by gift-wrap event id hex).
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `AcceptWelcome`) for the byte doorway.
    public static func marmotAcceptWelcome(
        correlationId: String,
        welcomeIdHex: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let widOffset = fbb.create(string: welcomeIdHex)
        let bodyStart = fbb.startTable(with: 1)
        fbb.add(offset: widOffset, at: 4) // slot 0: welcome_id_hex (required)
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(7), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Decline a pending MLS Welcome.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `DeclineWelcome`) for the byte doorway.
    public static func marmotDeclineWelcome(
        correlationId: String,
        welcomeIdHex: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let widOffset = fbb.create(string: welcomeIdHex)
        let bodyStart = fbb.startTable(with: 1)
        fbb.add(offset: widOffset, at: 4) // slot 0: welcome_id_hex (required)
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(8), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }

    /// Explicitly clear the pending-commit state for a MLS group.
    /// Builds the `nmp.marmot` `DispatchEnvelope` bytes (body `ClearPending`) for the byte doorway.
    public static func marmotClearPending(
        correlationId: String,
        groupIdHex: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let gidOffset = fbb.create(string: groupIdHex)
        let bodyStart = fbb.startTable(with: 1)
        fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)
        let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))
        let payloadStart = fbb.startTable(with: 3)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: UInt8(9), def: UInt8(0), at: 6) // slot 1: body_type
        fbb.add(offset: bodyOffset, at: 8) // slot 2: body
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NMMA")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.marmot",
            payload: payload
        )
    }
}
