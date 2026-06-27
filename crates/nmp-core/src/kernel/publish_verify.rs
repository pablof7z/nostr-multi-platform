//! Single well-formedness gate + shared publish helper for externally-supplied
//! signed events.
//!
//! Split from `publish_cmd.rs` so that file stays under the 500-LOC ceiling
//! (AGENTS.md / V-12).

use super::Kernel;
use crate::store::RawEvent;
use nmp_signer_iface::SignedEvent;

impl Kernel {
    /// Single well-formedness gate for an **externally-supplied** signed event
    /// entering the publish pipeline.
    ///
    /// "Well-formedness" is the cryptographic envelope check ONLY: the event id
    /// equals the SHA-256 hash of its canonical NIP-01 serialization, and the
    /// Schnorr signature is valid over that id — the same gate `kernel::ingest`
    /// applies to inbound events (`VerifiedEvent::try_from_raw`).
    ///
    /// **Opacity preserved (ADR-0025).** Validates the OUTER signed envelope
    /// only; it never decodes or validates NIP-specific inner shape. A
    /// gift-wrapped / Marmot event (kind:1059, kind:14) is opaque ciphertext
    /// under a well-formed signed envelope — accepted as long as the envelope's
    /// id-hash and signature verify, never inspected for inner semantics.
    ///
    /// **Single site for BOTH external ingress points.** The host-supplied
    /// pre-signed publish path (`actor/commands/publish.rs::publish_signed_event`)
    /// and the wasm/verbatim write path
    /// (`kernel_reducer/reply.rs::publish_signed_event`) both route untrusted
    /// bytes through here, so the verbatim/gift-wrap path is validated
    /// **identically** to the normal pre-signed path. Internally-signed
    /// publishes (`follow` / `publish_profile` / `publish_unsigned_event`) build
    /// the event from an `UnsignedEvent` and sign it with the kernel's own
    /// signer, so they are well-formed by construction and skip this gate.
    ///
    /// Fail-closed (D6): on a malformed/forged event it sets the categorized
    /// `ERR_MALFORMED_EVENT` toast (iOS branches on the discriminant rather than
    /// substring-matching English), records the matching `Failed` terminal under
    /// `correlation_id` when a dispatched action is waiting on it, and returns
    /// `Err(())`. The caller drops the event before any outbound frame or
    /// publish-queue entry is produced.
    /// Shared publish helper for **externally-signed** events (#2045 PR-A).
    ///
    /// Consolidates the three publish entry-points that previously each
    /// reimplemented the verify → D10 → route pipeline:
    ///
    /// 1. Native `actor::commands::publish::publish_signed_event` (primary).
    /// 2. `KernelReducer::publish_pre_signed` in `composition_seams.rs` (wasm
    ///    pre-signed path — previously skipped signature verification).
    ///
    /// **Pipeline (fail-closed at every step):**
    /// 1. `validate_publish_target` — empty/malformed explicit targets are
    ///    refused early (sets a toast + records `Failed` terminal).
    /// 2. `RawEvent` → `SignedEvent` reconstruction (no re-signing; id + sig
    ///    carried through verbatim).
    /// 3. `verify_externally_signed_event` — SHA-256 id-hash + Schnorr sig;
    ///    forged/garbled events are dropped before any outbound frame (D6).
    /// 4. `validate_publish_routing` (D10) — private/encrypted envelopes
    ///    (kind:1059, kind:14) with `PublishTarget::Auto` are refused.
    /// 5. `publish_signed_to_with_correlation` — NIP-65 outbox or explicit
    ///    relay-pin routing (D3).
    ///
    /// D6 — total: every error path returns `Vec::new()` (never a panic).
    pub(crate) fn publish_externally_signed(
        &mut self,
        raw: RawEvent,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    ) -> Vec<crate::relay::OutboundMessage> {
        use crate::publish::{
            target_is_explicit_nonempty, validate_publish_routing, validate_publish_target,
        };

        // Step 1 — target validation (inline fail_invalid_target logic).
        if let Err(reason) = validate_publish_target(&target) {
            let toast = format!("explicit publish target rejected: {reason}");
            self.set_last_error_toast(Some(toast.clone()));
            if let Some(id) = correlation_id {
                self.record_action_failure(id, toast);
            }
            return Vec::new();
        }
        // Step 2 — reconstruct the SignedEvent wire shape.
        let signed = SignedEvent {
            id: raw.id,
            sig: raw.sig,
            unsigned: nmp_signer_iface::UnsignedEvent {
                pubkey: raw.pubkey,
                kind: raw.kind,
                tags: raw.tags,
                content: raw.content,
                created_at: raw.created_at,
            },
        };
        // Step 3 — well-formedness: SHA-256 id-hash + Schnorr sig (D6).
        if self
            .verify_externally_signed_event(&signed, correlation_id.as_deref())
            .is_err()
        {
            return Vec::new();
        }
        // Step 4 — D10 routing-policy gate (private/encrypted envelope must
        // have an explicit non-empty relay pin).
        if let Err(reason) =
            validate_publish_routing(signed.unsigned.kind, target_is_explicit_nonempty(&target))
        {
            tracing::warn!(
                kind = signed.unsigned.kind,
                "publish_externally_signed refused: private/encrypted envelope without \
                 an explicit relay pin would leak through the author's public outbox (D10).",
            );
            self.set_last_error_toast(Some(reason.clone()));
            if let Some(id) = correlation_id {
                let code = crate::ui_token::codes::LIFECYCLE_PUBLISH_NO_EXPLICIT_TARGET;
                self.record_action_failure_coded(id, reason, Some(code), None);
            }
            return Vec::new();
        }
        // Step 5 — route through the publish engine.
        self.publish_signed_to_with_correlation(&signed, &[], target, correlation_id)
    }

    pub(crate) fn verify_externally_signed_event(
        &mut self,
        signed: &SignedEvent,
        correlation_id: Option<&str>,
    ) -> Result<(), ()> {
        let raw = RawEvent {
            id: signed.id.clone(),
            pubkey: signed.unsigned.pubkey.clone(),
            created_at: signed.unsigned.created_at,
            kind: signed.unsigned.kind,
            tags: signed.unsigned.tags.clone(),
            content: signed.unsigned.content.clone(),
            sig: signed.sig.clone(),
        };
        match crate::store::VerifiedEvent::try_from_raw(raw) {
            Ok(_) => Ok(()),
            Err(reason) => {
                let toast = format!("signed event rejected: {reason}");
                self.set_error_toast_with_category(
                    toast.clone(),
                    super::closed_reason::ERR_MALFORMED_EVENT,
                );
                if let Some(id) = correlation_id {
                    self.record_action_failure(id.to_string(), toast);
                }
                Err(())
            }
        }
    }
}
