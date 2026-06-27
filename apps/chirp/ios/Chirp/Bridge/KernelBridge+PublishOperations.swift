import Foundation

// ── Publish / action dispatch / social interaction operations ─────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).
// M14-1 (#2145): all social write verbs use GeneratedActionBuilders bytes —
// no namespace strings or JSON assembly in host code. App code NEVER spells a
// namespace or hand-assembles FlatBuffers; that lives only in generated code
// (ADR-0064 §3). Rust owns body shape + NIP-10/NIP-18 tag construction.

extension KernelHandle {
    /// Publish a kind:1 note (optionally a reply) through the generated builder.
    /// Swift supplies compose input only. A root note uses `publishRaw`
    /// (kind:1, no tags); a reply uses `publishReply`, where Rust derives the
    /// NIP-10 tags from the STORED parent event. A missing/invalid parent fails
    /// closed in Rust and surfaces as a `DispatchResult.failure` (D6).
    /// PR-A: returns the synchronous dispatch result so the caller can drive a
    /// spinner keyed on the correlation_id (or surface the error envelope to the
    /// user). The terminal verdict arrives through
    /// `projections["action_results"]` on a later snapshot tick — match by
    /// `correlation_id` to clear the spinner.
    @discardableResult
    func publishNote(content: String, replyTo: ChirpReplyTarget?) -> DispatchResult {
        let id = UUID().uuidString
        let bytes: [UInt8]
        if let replyTo {
            bytes = GeneratedActionBuilders.publishReply(
                correlationId: id,
                content: content,
                replyToEventId: replyTo.eventID
            )
        } else {
            bytes = GeneratedActionBuilders.publishRaw(
                correlationId: id,
                kind: 1,
                tags: [],
                content: content
            )
        }
        return dispatchBytes(bytes)
    }

    /// Publish a kind:6 repost of the given note through the generated builder.
    /// NIP-18: Rust derives the `["e", eventID]` / `["p", authorPubkey]` tags
    /// and empty content from the target facts.
    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        return dispatchBytes(GeneratedActionBuilders.repost(
            correlationId: id,
            targetEventId: eventID,
            targetKind: 1,
            targetAuthorPubkey: authorPubkey,
            relayHint: nil
        ))
    }

    func retryPublish(handle: String) {
        handle.withCString { nmp_app_retry_publish(raw, $0) }
    }

    /// Cancel an in-flight publish, addressed by the operation `correlationId`
    /// (S7/#1754). The outbox row's publish handle is also accepted (the kernel's
    /// handle↔correlation index self-maps it); the kernel records the
    /// user-initiated `cancelled` terminal under the ORIGINAL correlation_id.
    func cancelPublish(correlationID: String) {
        correlationID.withCString { nmp_app_cancel_action(raw, $0) }
    }

    @discardableResult
    func react(targetEventID: String, reaction: String) -> DispatchResult {
        let id = UUID().uuidString
        return dispatchBytes(GeneratedActionBuilders.react(
            correlationId: id,
            targetEventId: targetEventID,
            reaction: reaction,
            targetAuthorPubkey: nil
        ))
    }

    @discardableResult
    func follow(pubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        return dispatchBytes(GeneratedActionBuilders.follow(correlationId: id, pubkey: pubkey))
    }

    @discardableResult
    func unfollow(pubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        return dispatchBytes(GeneratedActionBuilders.unfollow(correlationId: id, pubkey: pubkey))
    }

    /// Dispatch a NIP-57 zap through the `nmp.nip57.zap` ActionModule.
    /// Rust signs the kind:9734 zap request, completes the two-leg LNURL-pay
    /// round-trip, and (when the `wallet` feature is active) auto-dispatches
    /// `ActorCommand::WalletPayInvoice` so the bolt11 → NWC pay loop closes
    /// without a second host round-trip. The shell never sees the bolt11
    /// or parses LNURL/kind:9734 — thin-shell rule (aim.md §6.9).
    ///
    /// `lnurl` is the pre-extracted value from the keyed profile sidecar.
    /// Relay selection stays kernel policy. PR-A: returns the
    /// synchronous dispatch envelope so the host can drive a spinner keyed
    /// on the minted correlation_id.
    @discardableResult
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64,
        comment: String? = nil
    ) -> DispatchResult {
        let id = UUID().uuidString
        return dispatchBytes(GeneratedActionBuilders.zap(
            correlationId: id,
            recipientPubkey: authorPubkey,
            amountMsats: amountMsats,
            // An empty lnurl must be ABSENT (not a present empty string): Rust's
            // zap `start()` rejects an explicitly-empty lnurl. When omitted the
            // kernel resolves it from the recipient's cached kind:0 (lud16/lud06).
            lnurl: lnurl.isEmpty ? nil : lnurl,
            relays: [],
            targetEventId: targetEventID,
            comment: comment
        ))
    }

    /// PR-G — acknowledge a `correlation_id` in the `action_stages` snapshot
    /// mirror so the kernel drops its stage history. The host calls this AFTER
    /// reacting to the terminal stage (`Accepted` / `Failed`) — until acked the
    /// entry persists on every snapshot, so a dropped tick cannot strand the
    /// progress indicator. Dispatch is non-blocking (D8). A null / unknown
    /// correlation_id is a silent no-op (D6).
    func ackActionStage(_ correlationId: String) {
        correlationId.withCString { nmp_app_ack_action_stage(raw, $0) }
    }
}
