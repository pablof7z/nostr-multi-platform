import Foundation

// ── Publish / action dispatch / social interaction operations ─────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    /// Publish a kind:1 note (optionally a reply) through the kernel's
    /// `ActionModule` family. Swift supplies compose input only; Rust builds
    /// the `nmp.publish` action spec, `PublishRaw` body, and any NIP-10 tags.
    /// PR-A: returns the synchronous dispatch result so the caller can drive a
    /// spinner keyed on the correlation_id (or surface the error envelope to the
    /// user). The terminal verdict arrives through
    /// `projections["action_results"]` on a later snapshot tick — match by
    /// `correlation_id` to clear the spinner.
    @discardableResult
    func publishNote(content: String, replyTo: ChirpReplyTarget?) -> DispatchResult {
        dispatchChirpIntent(.publishNote(content: content, replyTo: replyTo))
    }

    /// Publish a kind:6 repost of the given note through `PublishRaw`.
    /// NIP-18: tags `["e", eventID]` and `["p", authorPubkey]`, empty content.
    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        dispatchChirpIntent(.repost(eventID: eventID, authorPubkey: authorPubkey))
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
        dispatchChirpIntent(.react(eventID: targetEventID, reaction: reaction))
    }

    @discardableResult
    func follow(pubkey: String) -> DispatchResult {
        dispatchChirpIntent(.follow(pubkey: pubkey))
    }

    @discardableResult
    func unfollow(pubkey: String) -> DispatchResult {
        dispatchChirpIntent(.unfollow(pubkey: pubkey))
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
        dispatchChirpIntent(.zap(
            targetEventID: targetEventID,
            recipientPubkey: authorPubkey,
            amountMsats: amountMsats,
            lnurl: lnurl,
            comment: comment
        ))
    }

    /// Build and dispatch a Chirp action spec authored by Rust.
    ///
    /// Swift owns only raw user intent. Rust returns the exact namespace and
    /// body JSON before Rust encodes and dispatches typed bytes.
    @discardableResult
    func dispatchChirpIntent(_ intent: ChirpActionIntent) -> DispatchResult {
        let intentJson: String
        do {
            let data = try JSONEncoder().encode(intent)
            guard let json = String(data: data, encoding: .utf8) else {
                return .failure("failed to encode Chirp action intent as UTF-8")
            }
            intentJson = json
        } catch {
            return .failure("failed to encode Chirp action intent: \(error.localizedDescription)")
        }
        let envelope: String? = intentJson.withCString { intentPtr in
            guard let ptr = nmp_app_chirp_dispatch_intent_bytes(raw, intentPtr) else {
                return nil
            }
            defer { nmp_free_string(ptr) }
            return String(cString: ptr)
        }
        guard let envelope else {
            return .failure("intent dispatch returned a null envelope")
        }
        return DispatchResult.parse(envelope: envelope)
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
