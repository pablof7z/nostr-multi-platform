//! Single well-formedness gate for externally-supplied signed events.
//!
//! Split from `publish_cmd.rs` so that file stays under the 500-LOC ceiling
//! (AGENTS.md / V-12).

use super::Kernel;
use crate::store::RawEvent;
use crate::substrate::SignedEvent;

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
