//! Active-identity seeding + the #1753 S6 wasm signing capability round-trip
//! arms for [`super::WasmRuntime`].

use crate::protocol::{BeginSign, CapabilityFailure, DeliverSignerResponse, SetIdentity, WorkerEvent};
use crate::signer_slot;

use super::WasmRuntime;

impl WasmRuntime {
    /// Seed the kernel's active account from a [`SetIdentity`] identity request.
    ///
    /// Pure: no I/O, no JS-event-loop interaction. Validation failure surfaces
    /// as `CapabilityFailure` with a stable code (e.g. `unsupported_signer_kind`,
    /// `invalid_signer_pubkey`); success surfaces as `ActionAccepted` with
    /// `action_type = "nmp.set_identity"`.
    ///
    /// **No persistent signer is installed** (ADR-0064 §5): the request only
    /// validates + canonicalizes the pubkey and feeds it into the kernel via
    /// `set_active_account` so active-follows resolution and bootstrap interests
    /// know whose follows to load. Signing is the [`Self::begin_sign`] capability
    /// round-trip, never an `Arc<dyn Signer>` awaited inside a publish flow.
    pub(super) fn set_identity(&mut self, request: SetIdentity) -> Vec<WorkerEvent> {
        match signer_slot::canonical_pubkey_from_request(&request) {
            Ok(canonical_pubkey) => {
                // Use the canonical lowercase hex from the parsed key, not the
                // raw wire string. Uppercase input must not seed a non-canonical
                // active_account (B2).
                let outbound = self
                    .reducer
                    .borrow_mut()
                    .set_active_account(canonical_pubkey);
                self.fan_outbound(outbound);
                self.accepted_with_snapshot("nmp.set_identity".to_string(), request.correlation_id)
            }
            Err(error) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: "nmp.set_identity".to_string(),
                correlation_id: request.correlation_id,
                reason: error.detail(),
            })],
        }
    }

    /// #1753 S6 — begin a NIP-07 sign capability round-trip. Parks a sign op in
    /// the reducer's shared `ParkedSignerOps` queue (the SAME component the
    /// native actor loop drives) and emits the [`WorkerEvent::SignRequest`] the
    /// main-thread broker fulfils. Total (D6): a malformed unsigned JSON parks
    /// nothing and surfaces a [`WorkerEvent::SignFailed`].
    pub(super) fn begin_sign(&mut self, request: BeginSign) -> Vec<WorkerEvent> {
        match self
            .reducer
            .borrow_mut()
            .begin_sign_roundtrip(request.account_pubkey, &request.unsigned_json)
        {
            Ok(req) => vec![WorkerEvent::SignRequest {
                correlation_id: req.correlation_id,
                account_pubkey: req.account_pubkey,
                unsigned_json: req.unsigned_json,
            }],
            // No correlation id was minted (begin failed before parking); echo
            // an empty id so the host can still surface the failure.
            Err(reason) => vec![WorkerEvent::SignFailed {
                correlation_id: String::new(),
                reason,
            }],
        }
    }

    /// #1753 S6 — deliver a signer response from the main-thread broker. THIS is
    /// the message re-entry: it drives the parked sign op exactly once, here,
    /// from the inbound message — no polling (D8). Account-pinned: the reducer
    /// rejects a signature authored by a different account than the round-trip
    /// was begun for.
    pub(super) fn deliver_signer_response(
        &mut self,
        response: DeliverSignerResponse,
    ) -> Vec<WorkerEvent> {
        use nmp_core::SignRoundTripOutcome;
        let outcome = {
            let mut reducer = self.reducer.borrow_mut();
            match (response.signed_json, response.error) {
                // A broker-reported failure (user rejected / no window.nostr).
                (_, Some(error)) => reducer.fail_sign_roundtrip(&response.correlation_id, &error),
                // A signed event to deliver.
                (Some(signed_json), None) => {
                    reducer.deliver_signed_response(&response.correlation_id, &signed_json)
                }
                // Neither field set — an honest protocol error, failed closed.
                (None, None) => reducer.fail_sign_roundtrip(
                    &response.correlation_id,
                    "deliver_signer_response carried neither signed_json nor error",
                ),
            }
        };
        match outcome {
            SignRoundTripOutcome::Completed {
                correlation_id,
                signed_json,
            } => vec![WorkerEvent::SignCompleted {
                correlation_id,
                signed_json,
            }],
            SignRoundTripOutcome::Failed {
                correlation_id,
                reason,
            } => vec![WorkerEvent::SignFailed {
                correlation_id,
                reason,
            }],
            SignRoundTripOutcome::Unknown { correlation_id } => vec![WorkerEvent::SignFailed {
                correlation_id,
                reason: "no parked sign round-trip matched this correlation id (stale or \
                         duplicate delivery)"
                    .to_string(),
            }],
        }
    }
}
