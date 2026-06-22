// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform swift \
//       --out ios/Chirp/Chirp/Bridge/Generated/ActionBuilders.generated.swift
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
}
