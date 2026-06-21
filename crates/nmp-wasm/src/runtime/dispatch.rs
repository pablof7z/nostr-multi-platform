//! Action-dispatch arm of [`super::WasmRuntime::handle`].
//!
//! Split out of `runtime.rs` (LOC ceiling) — the synchronous `dispatch`
//! router plus its two helpers (`app_action`, `accepted_with_snapshot`) are a
//! cohesive unit: they translate a host `ActionDispatch` / `AppAction` into the
//! `KernelReducer` mutation + the `[ActionAccepted, UpdateBytes?]` reply. The
//! relay-driven snapshot push and the `Start`/`Stop`/`SetSigner` arms stay in
//! `runtime.rs`; only the action-namespace routing lives here.
//!
//! The methods are defined on `impl super::WasmRuntime` so they remain ordinary
//! private methods of the runtime — the file boundary is a size-management
//! seam, not an API boundary.

use crate::dispatch_routing::{
    claim_dispatch_from_action, execute_claim_dispatch, execute_interest_dispatch,
    interest_dispatch_from_action, kernel_action_from_dispatch, write_path_unavailable_reason,
};
use crate::protocol::{ActionDispatch, AppAction, CapabilityFailure, WorkerEvent};
use nmp_core::dispatch_envelope::{decode_dispatch_envelope, DecodedDispatch};
use nmp_core::KernelUpdate;

use super::{WasmRuntime, WasmRuntimeError};

impl WasmRuntime {
    /// ADR-0064 / S2 (#1750) — the **binary write doorway**.
    ///
    /// The host posts the write command as a transferable `Uint8Array` (NOT a
    /// JSON number array): the raw bytes of a finished `DispatchEnvelope`. This
    /// method is the wasm half of the one byte transport — the native FFI half
    /// is `nmp_app_dispatch_action_bytes(app, ptr, len)`; both decode through the
    /// SAME `nmp_core::dispatch_envelope::decode_dispatch_envelope` path.
    ///
    /// Fail-closed: a decode rejection (bad file identifier, schema_version
    /// tripwire mismatch, oversize, missing routing fields) surfaces as a
    /// data-shaped `WorkerEvent::Error` with the RAW reason (D6) — never a panic,
    /// never a silent accept. On success it routes by `action_namespace` behind
    /// the existing one doorway, carrying the OPAQUE payload verbatim. The typed
    /// per-crate payload decode is S3's job (#1751); this method never peeks
    /// inside `payload`.
    pub fn dispatch_bytes(&mut self, bytes: &[u8]) -> Vec<WorkerEvent> {
        let decoded = match decode_dispatch_envelope(bytes) {
            Ok(decoded) => decoded,
            Err(err) => {
                // Fail closed: the decode rejected. Surface the RAW discriminant
                // as a data-shaped error; correlation_id is unknown (the buffer
                // never decoded far enough to trust it).
                return vec![WorkerEvent::Error {
                    code: "dispatch_envelope_rejected".to_string(),
                    message: err.to_string(),
                    correlation_id: None,
                }];
            }
        };
        self.route_decoded_dispatch(decoded)
    }

    /// Route a gate-passed [`DecodedDispatch`] by `action_namespace` behind the
    /// one doorway. The OPAQUE payload is carried verbatim; namespaces whose
    /// typed payload decode is not yet wired (S3 / #1751) acknowledge through the
    /// same honest write-path reason as the JSON `dispatch` arm rather than
    /// interpreting the bytes.
    fn route_decoded_dispatch(&mut self, decoded: DecodedDispatch) -> Vec<WorkerEvent> {
        let DecodedDispatch {
            correlation_id,
            action_namespace,
            payload: _opaque,
        } = decoded;
        // The opaque payload is NOT interpreted here — S3 teaches the registry to
        // decode the typed FlatBuffers root. For S2 the binary doorway routes by
        // namespace and surfaces the same honest write-path reason the JSON arm
        // does for app-level writes, proving the envelope crossed and decoded.
        vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_namespace,
            correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })]
    }

    /// Build an `[ActionAccepted, UpdateBytes]` pair for a successful
    /// synchronous dispatch. Used by every arm that fans outbound and then
    /// returns the standard acknowledgement + snapshot.
    pub(super) fn accepted_with_snapshot(
        &mut self,
        action_type: String,
        correlation_id: String,
    ) -> Vec<WorkerEvent> {
        vec![
            WorkerEvent::ActionAccepted { action_type, correlation_id },
            self.snapshot_event(),
        ]
    }

    pub(super) fn app_action(
        &mut self,
        action: AppAction,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        let (action_type, _payload) = action.into_dispatch_parts();
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }

    pub(super) fn dispatch(
        &mut self,
        action: ActionDispatch,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // F-CR-00 claim arm: claim/release refcounts (see execute_claim_dispatch
        // in dispatch_routing.rs for the full rationale / `can_send` contract).
        if let Some(claim) = claim_dispatch_from_action(&action) {
            let can_send = self.reducer.borrow().any_relay_connected();
            let outbound = execute_claim_dispatch(&mut self.reducer.borrow_mut(), claim, can_send);
            self.fan_outbound(outbound);
            // Claim/release are refcount bookkeeping — they carry no new
            // user-visible data of their own (the resolved kind:0 arrives later
            // via the relay-pool ingest sink, which pushes its OWN snapshot).
            // Pushing a snapshot here hands the reactive web host a fresh frame
            // on every claim; the host's feed `<For>` rebuilds its rows, which
            // remounts the avatar/name components, which release + re-claim —
            // an unbounded claim → snapshot → re-render → claim loop that, on
            // the single-threaded wasm worker, floods the main thread with
            // snapshot frames and starves (or OOM-crashes) the UI so the feed
            // never paints (feed.spec.ts toBeVisible timeout). Only ACK the
            // action; let the data-bearing ingest frame drive the next render.
            return Ok(vec![WorkerEvent::ActionAccepted {
                action_type: action.action_type,
                correlation_id: action.correlation_id,
            }]);
        }
        // Feed-verb arm: open/close generic interests + active-follows.
        if let Some(interest) = interest_dispatch_from_action(&action) {
            let outbound = execute_interest_dispatch(&mut self.reducer.borrow_mut(), interest);
            self.fan_outbound(outbound);
            return Ok(self.accepted_with_snapshot(action.action_type, action.correlation_id));
        }
        // Kernel-namespace actions (`nmp.kernel.start`, `open_uri`, etc.) map
        // to `KernelAction` variants and run through `KernelReducer::reduce`.
        if let Some(kernel_action) = kernel_action_from_dispatch(&action) {
            let update = self.reducer.borrow_mut().reduce(kernel_action);
            match update {
                KernelUpdate::Started { .. } => { self.meta.borrow_mut().started = true; }
                KernelUpdate::Stopped { .. } => { self.meta.borrow_mut().started = false; }
                _ => {}
            }
            return Ok(self.accepted_with_snapshot(action.action_type, action.correlation_id));
        }
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action.action_type,
            correlation_id: action.correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }
}
