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
}
