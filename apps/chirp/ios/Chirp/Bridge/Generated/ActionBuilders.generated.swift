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
}
