import Foundation

// ── Publish / action dispatch / social interaction operations ─────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).
// M14-1 / PR2 (#2145): every social write uses GeneratedActionBuilders bytes
// dispatched via `dispatchBytes` — no namespace strings, no JSON assembly, no
// tag construction in host code. Rust owns all protocol-tag construction.

extension KernelHandle {
    /// Publish a kind:1 note (optionally a reply) via the typed FlatBuffers byte
    /// builder (`nmp.nip01.publish_note`). Swift supplies compose input only; the
    /// Rust action module builds the kind:1 event and any NIP-10 reply tags from
    /// the parent fields. Returns the synchronous dispatch result so the caller
    /// can drive a spinner keyed on the correlation_id; the terminal verdict
    /// arrives through the `action_lifecycle` projection on a later snapshot tick.
    @discardableResult
    func publishNote(content: String, replyTo: ChirpReplyTarget?) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.publishNote(
            correlationId: id,
            content: content,
            replyEventId: replyTo?.eventID,
            replyAuthorPubkey: replyTo?.authorPubkey,
            replyRootEventId: nil,
            replyRootRelay: nil,
            replyMentionedPubkeys: nil
        )
        return dispatchBytes(bytes)
    }

    /// Publish a kind:6 repost of the given note via the typed FlatBuffers byte
    /// builder (`nmp.nip18.repost`). Rust builds the kind:6 event with tags
    /// `["e", eventID]` and `["p", authorPubkey]` and empty content.
    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.repost(
            correlationId: id, eventId: eventID, authorPubkey: authorPubkey)
        return dispatchBytes(bytes)
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
        let bytes = GeneratedActionBuilders.react(
            correlationId: id,
            targetEventId: targetEventID,
            reaction: reaction,
            targetAuthorPubkey: nil
        )
        return dispatchBytes(bytes)
    }

    @discardableResult
    func follow(pubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.follow(correlationId: id, pubkey: pubkey)
        return dispatchBytes(bytes)
    }

    @discardableResult
    func unfollow(pubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.unfollow(correlationId: id, pubkey: pubkey)
        return dispatchBytes(bytes)
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
        // V-07: relay selection is kernel policy — pass an empty `relays` list;
        // the actor auto-selects from the recipient's kind:10002 write/both set.
        let bytes = GeneratedActionBuilders.zap(
            correlationId: id,
            recipientPubkey: authorPubkey,
            amountMsats: amountMsats,
            lnurl: lnurl,
            relays: [],
            targetEventId: targetEventID,
            comment: comment
        )
        return dispatchBytes(bytes)
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
