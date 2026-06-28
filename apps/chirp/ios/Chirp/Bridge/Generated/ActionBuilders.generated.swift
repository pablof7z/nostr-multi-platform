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

    /// Publish a NIP-84 kind:9802 highlight annotation.
    /// Builds the `nmp.nip84.publish_highlight` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishHighlight(
        correlationId: String,
        content: String,
        context: String?,
        sourceEventId: String?,
        sourceAddress: String?,
        sourceAuthorPubkey: String?,
        alt: String?,
        externalIds: [String]?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let contentOffset = fbb.create(string: content)
        let contextOffset: Offset = context.map { fbb.create(string: $0) } ?? Offset()
        let sourceEventIdOffset: Offset = sourceEventId.map { fbb.create(string: $0) } ?? Offset()
        let sourceAddressOffset: Offset = sourceAddress.map { fbb.create(string: $0) } ?? Offset()
        let sourceAuthorPubkeyOffset: Offset = sourceAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let altOffset: Offset = alt.map { fbb.create(string: $0) } ?? Offset()
        let externalIdsOffset: Offset = {
            guard let values = externalIds, !values.isEmpty else { return Offset() }
            let offsets = values.map { fbb.create(string: $0) }
            return fbb.createVector(ofOffsets: offsets)
        }()
        let payloadStart = fbb.startTable(with: 8)
        fbb.add(element: UInt32(2), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: contentOffset, at: 6) // slot 1: content
        if contextOffset.o != 0 { fbb.add(offset: contextOffset, at: 8) } // slot 2: context
        if sourceEventIdOffset.o != 0 { fbb.add(offset: sourceEventIdOffset, at: 10) } // slot 3: sourceEventId
        if sourceAddressOffset.o != 0 { fbb.add(offset: sourceAddressOffset, at: 12) } // slot 4: sourceAddress
        if sourceAuthorPubkeyOffset.o != 0 { fbb.add(offset: sourceAuthorPubkeyOffset, at: 14) } // slot 5: sourceAuthorPubkey
        if altOffset.o != 0 { fbb.add(offset: altOffset, at: 16) } // slot 6: alt
        if externalIdsOffset.o != 0 { fbb.add(offset: externalIdsOffset, at: 18) } // slot 7: externalIds
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N84H")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip84.publish_highlight",
            payload: payload
        )
    }

    /// Publish a NIP-22 kind:1111 comment.
    /// Builds the `nmp.nip22.post_comment` `DispatchEnvelope` bytes for the byte doorway.
    public static func postComment(
        correlationId: String,
        rootTagName: String,
        rootTagValue: String,
        rootKind: UInt32,
        parentEventId: String?,
        rootAuthorPubkey: String?,
        parentAuthorPubkey: String?,
        content: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let rootTagNameOffset = fbb.create(string: rootTagName)
        let rootTagValueOffset = fbb.create(string: rootTagValue)
        let parentEventIdOffset: Offset = parentEventId.map { fbb.create(string: $0) } ?? Offset()
        let rootAuthorPubkeyOffset: Offset = rootAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let parentAuthorPubkeyOffset: Offset = parentAuthorPubkey.map { fbb.create(string: $0) } ?? Offset()
        let contentOffset = fbb.create(string: content)
        let payloadStart = fbb.startTable(with: 8)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: rootTagNameOffset, at: 6) // slot 1: rootTagName
        fbb.add(offset: rootTagValueOffset, at: 8) // slot 2: rootTagValue
        fbb.add(element: UInt32(rootKind), def: UInt32(0), at: 10) // slot 3: rootKind
        if parentEventIdOffset.o != 0 { fbb.add(offset: parentEventIdOffset, at: 12) } // slot 4: parentEventId
        if rootAuthorPubkeyOffset.o != 0 { fbb.add(offset: rootAuthorPubkeyOffset, at: 14) } // slot 5: rootAuthorPubkey
        if parentAuthorPubkeyOffset.o != 0 { fbb.add(offset: parentAuthorPubkeyOffset, at: 16) } // slot 6: parentAuthorPubkey
        fbb.add(offset: contentOffset, at: 18) // slot 7: content
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N22C")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip22.post_comment",
            payload: payload
        )
    }

    /// Add an item to a NIP-51 kind:30003 bookmark or kind:30004 curation set.
    /// Builds the `nmp.nip51.add_bookmark_set_item` `DispatchEnvelope` bytes for the byte doorway.
    public static func addBookmarkSetItem(
        correlationId: String,
        accountPubkey: String,
        setKind: UInt8,
        identifier: String,
        itemKind: UInt8,
        value: String,
        relay: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let identifierOffset = fbb.create(string: identifier)
        let valueOffset = fbb.create(string: value)
        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()
        let itemStart = fbb.startTable(with: 3)
        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind
        fbb.add(offset: valueOffset, at: 6) // slot 1: value
        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay
        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))
        let payloadStart = fbb.startTable(with: 5)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey
        fbb.add(element: setKind, def: UInt8(0), at: 8) // slot 2: set_kind
        fbb.add(offset: identifierOffset, at: 10) // slot 3: identifier
        fbb.add(offset: itemRoot, at: 12) // slot 4: item
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N51S")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.add_bookmark_set_item",
            payload: payload
        )
    }

    /// Remove an item from a NIP-51 kind:30003 bookmark or kind:30004 curation set.
    /// Builds the `nmp.nip51.remove_bookmark_set_item` `DispatchEnvelope` bytes for the byte doorway.
    public static func removeBookmarkSetItem(
        correlationId: String,
        accountPubkey: String,
        setKind: UInt8,
        identifier: String,
        itemKind: UInt8,
        value: String,
        relay: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let identifierOffset = fbb.create(string: identifier)
        let valueOffset = fbb.create(string: value)
        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()
        let itemStart = fbb.startTable(with: 3)
        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind
        fbb.add(offset: valueOffset, at: 6) // slot 1: value
        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay
        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))
        let payloadStart = fbb.startTable(with: 5)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey
        fbb.add(element: setKind, def: UInt8(0), at: 8) // slot 2: set_kind
        fbb.add(offset: identifierOffset, at: 10) // slot 3: identifier
        fbb.add(offset: itemRoot, at: 12) // slot 4: item
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N51S")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.remove_bookmark_set_item",
            payload: payload
        )
    }

    /// Publish or update a NIP-B0 kind:39701 web bookmark.
    /// Builds the `nmp.nip51.publish_web_bookmark` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishWebBookmark(
        correlationId: String,
        accountPubkey: String,
        url: String,
        title: String?,
        description: String?,
        publishedAt: UInt64?,
        hashtags: [String]?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let accountPubkeyOffset = fbb.create(string: accountPubkey)
        let urlOffset = fbb.create(string: url)
        let titleOffset: Offset = title.map { fbb.create(string: $0) } ?? Offset()
        let descriptionOffset: Offset = description.map { fbb.create(string: $0) } ?? Offset()
        let hashtagsOffset: Offset = {
            guard let values = hashtags, !values.isEmpty else { return Offset() }
            let offsets = values.map { fbb.create(string: $0) }
            return fbb.createVector(ofOffsets: offsets)
        }()
        let payloadStart = fbb.startTable(with: 8)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: accountPubkey
        fbb.add(offset: urlOffset, at: 8) // slot 2: url
        if titleOffset.o != 0 { fbb.add(offset: titleOffset, at: 10) } // slot 3: title
        if descriptionOffset.o != 0 { fbb.add(offset: descriptionOffset, at: 12) } // slot 4: description
        if let publishedAtVal = publishedAt {
            fbb.add(element: publishedAtVal, def: UInt64(0), at: 14) // slot 5: publishedAt
            fbb.add(element: true, def: false, at: 16) // slot 6: hasPublishedAt
        }
        if hashtagsOffset.o != 0 { fbb.add(offset: hashtagsOffset, at: 18) } // slot 7: hashtags
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N51W")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip51.publish_web_bookmark",
            payload: payload
        )
    }

    /// Upload a file via BUD-02 to one or more Blossom servers.
    /// Builds the `nmp.blossom.upload` `DispatchEnvelope` bytes for the byte doorway.
    public static func blossomUpload(
        correlationId: String,
        filePath: String,
        contentType: String?,
        servers: [String]?,
        signerPubkey: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let filePathOffset = fbb.create(string: filePath)
        let contentTypeOffset: Offset = contentType.map { fbb.create(string: $0) } ?? Offset()
        let serversOffset: Offset = {
            guard let values = servers, !values.isEmpty else { return Offset() }
            let offsets = values.map { fbb.create(string: $0) }
            return fbb.createVector(ofOffsets: offsets)
        }()
        let signerPubkeyOffset: Offset = signerPubkey.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 5)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: filePathOffset, at: 6) // slot 1: filePath
        if contentTypeOffset.o != 0 { fbb.add(offset: contentTypeOffset, at: 8) } // slot 2: contentType
        if serversOffset.o != 0 { fbb.add(offset: serversOffset, at: 10) } // slot 3: servers
        if signerPubkeyOffset.o != 0 { fbb.add(offset: signerPubkeyOffset, at: 12) } // slot 4: signerPubkey
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "BUPL")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.blossom.upload",
            payload: payload
        )
    }

    /// Claim or release the tailing interest for a note's visible relations (NIP-01).
    /// Builds the `nmp.nip01.visible_note_relations` `DispatchEnvelope` bytes for the byte doorway.
    public static func visibleNoteRelations(
        correlationId: String,
        op: UInt8,
        eventId: String,
        consumerId: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let eventIdOffset = fbb.create(string: eventId)
        let consumerIdOffset = fbb.create(string: consumerId)
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: op, def: UInt8(0), at: 6) // slot 1: op
        fbb.add(offset: eventIdOffset, at: 8) // slot 2: eventId
        fbb.add(offset: consumerIdOffset, at: 10) // slot 3: consumerId
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NR01")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip01.visible_note_relations",
            payload: payload
        )
    }

    /// Open or close a relay-pinned browse subscription.
    /// Builds the `nmp.browse_relay` `DispatchEnvelope` bytes for the byte doorway.
    public static func browseRelay(
        correlationId: String,
        op: UInt8,
        relayUrl: String?,
        kinds: [UInt32]?,
        lifecycle: UInt8,
        interestId: UInt64
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let relayUrlOffset: Offset = relayUrl.map { fbb.create(string: $0) } ?? Offset()
        let kindsOffset: Offset = {
            guard let values = kinds, !values.isEmpty else { return Offset() }
            return fbb.createVector(values)
        }()
        let payloadStart = fbb.startTable(with: 6)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: op, def: UInt8(0), at: 6) // slot 1: op
        if relayUrlOffset.o != 0 { fbb.add(offset: relayUrlOffset, at: 8) } // slot 2: relayUrl
        if kindsOffset.o != 0 { fbb.add(offset: kindsOffset, at: 10) } // slot 3: kinds
        fbb.add(element: lifecycle, def: UInt8(0), at: 12) // slot 4: lifecycle
        fbb.add(element: interestId, def: UInt64(0), at: 14) // slot 5: interestId
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NBRW")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.browse_relay",
            payload: payload
        )
    }

    /// Claim or release a NIP-23 topic-articles subscription.
    /// Builds the `nmp.app.topic_articles` `DispatchEnvelope` bytes for the byte doorway.
    public static func topicArticles(
        correlationId: String,
        op: UInt8,
        topic: String,
        consumerId: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let topicOffset = fbb.create(string: topic)
        let consumerIdOffset = fbb.create(string: consumerId)
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(element: op, def: UInt8(0), at: 6) // slot 1: op
        fbb.add(offset: topicOffset, at: 8) // slot 2: topic
        fbb.add(offset: consumerIdOffset, at: 10) // slot 3: consumerId
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "NTPC")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.app.topic_articles",
            payload: payload
        )
    }

    /// Discover NIP-29 groups hosted on a relay.
    /// Builds the `nmp.nip29.discover` `DispatchEnvelope` bytes for the byte doorway.
    public static func discoverGroups(
        correlationId: String,
        relayUrl: String
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let relayUrlOffset = fbb.create(string: relayUrl)
        let payloadStart = fbb.startTable(with: 2)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: relayUrlOffset, at: 6) // slot 1: relayUrl
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N29D")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip29.discover",
            payload: payload
        )
    }

    /// Publish an event to a NIP-29 group (any kind).
    /// Builds the `nmp.nip29.publish_group_event` `DispatchEnvelope` bytes for the byte doorway.
    public static func publishGroupEvent(
        correlationId: String,
        group: (hostRelayUrl: String, localId: String),
        kind: UInt32,
        content: String?,
        tags: [[String]]?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let groupHostRelayUrlOffset = fbb.create(string: group.hostRelayUrl)
        let groupLocalIdOffset = fbb.create(string: group.localId)
        let groupTableStart = fbb.startTable(with: 2)
        fbb.add(offset: groupHostRelayUrlOffset, at: 4) // GroupRef slot 0: host_relay_url
        fbb.add(offset: groupLocalIdOffset, at: 6)       // GroupRef slot 1: local_id
        let groupOffset = Offset(offset: fbb.endTable(at: groupTableStart))
        let contentOffset: Offset = content.map { fbb.create(string: $0) } ?? Offset()
        let tagsOffset: Offset = {
            guard let tagRows = tags, !tagRows.isEmpty else { return Offset() }
            var tagOffsets: [Offset] = []
            for row in tagRows {
                let valOffsets = row.map { fbb.create(string: $0) }
                let valsVec = fbb.createVector(ofOffsets: valOffsets)
                let tagStart = fbb.startTable(with: 1)
                fbb.add(offset: valsVec, at: 4) // StringTag slot 0: values
                tagOffsets.append(Offset(offset: fbb.endTable(at: tagStart)))
            }
            return fbb.createVector(ofOffsets: tagOffsets)
        }()
        let payloadStart = fbb.startTable(with: 5)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: groupOffset, at: 6) // slot 1: group
        fbb.add(element: UInt32(kind), def: UInt32(0), at: 8) // slot 2: kind
        if contentOffset.o != 0 { fbb.add(offset: contentOffset, at: 10) } // slot 3: content
        if tagsOffset.o != 0 { fbb.add(offset: tagsOffset, at: 12) } // slot 4: tags
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N29G")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip29.publish_group_event",
            payload: payload
        )
    }

    /// Request membership in a NIP-29 group.
    /// Builds the `nmp.nip29.join` `DispatchEnvelope` bytes for the byte doorway.
    public static func joinGroup(
        correlationId: String,
        group: (hostRelayUrl: String, localId: String),
        inviteCode: String?,
        reason: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let groupHostRelayUrlOffset = fbb.create(string: group.hostRelayUrl)
        let groupLocalIdOffset = fbb.create(string: group.localId)
        let groupTableStart = fbb.startTable(with: 2)
        fbb.add(offset: groupHostRelayUrlOffset, at: 4) // GroupRef slot 0: host_relay_url
        fbb.add(offset: groupLocalIdOffset, at: 6)       // GroupRef slot 1: local_id
        let groupOffset = Offset(offset: fbb.endTable(at: groupTableStart))
        let inviteCodeOffset: Offset = inviteCode.map { fbb.create(string: $0) } ?? Offset()
        let reasonOffset: Offset = reason.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 4)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: groupOffset, at: 6) // slot 1: group
        if inviteCodeOffset.o != 0 { fbb.add(offset: inviteCodeOffset, at: 8) } // slot 2: inviteCode
        if reasonOffset.o != 0 { fbb.add(offset: reasonOffset, at: 10) } // slot 3: reason
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N29J")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip29.join",
            payload: payload
        )
    }

    /// Create a new public NIP-29 group.
    /// Builds the `nmp.nip29.create_public_group` `DispatchEnvelope` bytes for the byte doorway.
    public static func createPublicGroup(
        correlationId: String,
        group: (hostRelayUrl: String, localId: String),
        name: String,
        about: String?,
        picture: String?,
        visibility: Int8,
        access: Int8,
        parent: String?
    ) -> [UInt8] {
        var fbb = FlatBufferBuilder()
        let groupHostRelayUrlOffset = fbb.create(string: group.hostRelayUrl)
        let groupLocalIdOffset = fbb.create(string: group.localId)
        let groupTableStart = fbb.startTable(with: 2)
        fbb.add(offset: groupHostRelayUrlOffset, at: 4) // GroupRef slot 0: host_relay_url
        fbb.add(offset: groupLocalIdOffset, at: 6)       // GroupRef slot 1: local_id
        let groupOffset = Offset(offset: fbb.endTable(at: groupTableStart))
        let nameOffset = fbb.create(string: name)
        let aboutOffset: Offset = about.map { fbb.create(string: $0) } ?? Offset()
        let pictureOffset: Offset = picture.map { fbb.create(string: $0) } ?? Offset()
        let parentOffset: Offset = parent.map { fbb.create(string: $0) } ?? Offset()
        let payloadStart = fbb.startTable(with: 8)
        fbb.add(element: UInt32(1), def: UInt32(0), at: 4) // slot 0: schema_version
        fbb.add(offset: groupOffset, at: 6) // slot 1: group
        fbb.add(offset: nameOffset, at: 8) // slot 2: name
        if aboutOffset.o != 0 { fbb.add(offset: aboutOffset, at: 10) } // slot 3: about
        if pictureOffset.o != 0 { fbb.add(offset: pictureOffset, at: 12) } // slot 4: picture
        fbb.add(element: visibility, def: Int8(0), at: 14) // slot 5: visibility
        fbb.add(element: access, def: Int8(0), at: 16) // slot 6: access
        if parentOffset.o != 0 { fbb.add(offset: parentOffset, at: 18) } // slot 7: parent
        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))
        fbb.finish(offset: payloadRoot, fileId: "N29P")
        let payload = fbb.sizedByteArray
        return encodeDispatchEnvelope(
            correlationId: correlationId,
            actionNamespace: "nmp.nip29.create_public_group",
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
