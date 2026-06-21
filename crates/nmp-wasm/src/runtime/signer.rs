//! Signer installation helper for [`super::WasmRuntime`].

use crate::protocol::{CapabilityFailure, SetSigner, WorkerEvent};
use crate::signer_slot;

use super::WasmRuntime;

impl WasmRuntime {
    /// V-01 Stage 3b - install a signer from a [`SetSigner`] request.
    ///
    /// Pure: no I/O, no JS-event-loop interaction. Construction failure
    /// surfaces as `CapabilityFailure` with a stable code (e.g.
    /// `unsupported_signer_kind`, `invalid_signer_pubkey`); success surfaces
    /// as `ActionAccepted` with `action_type = "nmp.set_signer"`.
    ///
    /// PR-3 viewer-pubkey hand-off: on success the pubkey from the signer
    /// request is fed into the kernel via `set_active_account` so
    /// active-follows resolution and bootstrap interests know whose follows
    /// to load without waiting for a separate `set_active_account` action.
    pub(super) fn set_signer(&mut self, request: SetSigner) -> Vec<WorkerEvent> {
        match signer_slot::install_from_request(&request) {
            Ok((signer, canonical_pubkey)) => {
                self.signer = Some(signer);
                // Use the canonical lowercase hex from the parsed key, not the
                // raw wire string. Uppercase input must not seed a non-canonical
                // active_account (B2).
                let outbound = self
                    .reducer
                    .borrow_mut()
                    .set_active_account(canonical_pubkey);
                self.fan_outbound(outbound);
                self.accepted_with_snapshot("nmp.set_signer".to_string(), request.correlation_id)
            }
            Err(error) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: "nmp.set_signer".to_string(),
                correlation_id: request.correlation_id,
                reason: error.detail(),
            })],
        }
    }
}
