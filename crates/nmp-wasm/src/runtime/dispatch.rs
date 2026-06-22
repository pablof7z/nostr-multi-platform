//! Action-dispatch arm of [`super::WasmRuntime::handle`].
//!
//! Split out of `runtime.rs` (LOC ceiling) — the binary `dispatch_bytes`
//! doorway and the legacy JSON `dispatch` router plus `accepted_with_snapshot`
//! are a cohesive unit: they translate a host write command into the
//! `KernelReducer` mutation + the `[ActionAccepted, UpdateBytes?]` reply. The
//! relay-driven snapshot push and the `Start`/`Stop`/`SetSigner` arms stay in
//! `runtime.rs`; only the action-namespace routing lives here.
//!
//! The methods are defined on `impl super::WasmRuntime` so they remain ordinary
//! private methods of the runtime — the file boundary is a size-management
//! seam, not an API boundary.

use crate::dispatch_routing::{
    execute_ref_dispatch, kernel_action_from_dispatch, ref_dispatch_from_action,
    write_path_unavailable_reason,
};
use crate::protocol::{ActionDispatch, CapabilityFailure, WorkerEvent};
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
        // Publishing itself stays honestly-disabled until the web composition
        // root wires a real `OutboxResolver` (#1007); ADR-0064 §5 signing is the
        // `BeginSign` capability round-trip, never an in-flow `Arc<dyn Signer>`.
        let reason = write_path_unavailable_reason(self.has_active_account());
        vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_namespace,
            correlation_id,
            reason,
        })]
    }

    /// Whether the kernel has an active account seeded (via `SetSigner` /
    /// `set_active_account`). The two honest write-unavailability states key on
    /// this instead of a persistent signer slot (removed in #1743 Cut A,
    /// ADR-0064 §5): no account → `signer_not_installed`; account seeded but the
    /// web preview has no outbox resolver → `publish_not_supported_in_web_preview`.
    fn has_active_account(&self) -> bool {
        self.reducer.borrow().active_account_pubkey().is_some()
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

    pub(super) fn dispatch(
        &mut self,
        action: ActionDispatch,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // ADR-0063 reference-resolution arm: resolve/release refcounts via the
        // unified seam (see execute_ref_dispatch in dispatch_routing.rs for the
        // full rationale / `can_send` contract).
        if let Some(ref_dispatch) = ref_dispatch_from_action(&action) {
            let can_send = self.reducer.borrow().any_relay_connected();
            let outbound =
                execute_ref_dispatch(&mut self.reducer.borrow_mut(), ref_dispatch, can_send);
            self.fan_outbound(outbound);
            // Resolve/release are refcount bookkeeping — they carry no new
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
        // #1740 step 8: the raw feed-verb dispatch arm (`nmp.kernel.open_interest`
        // / `close_interest` and `nmp.feed.declare_active_follows` /
        // `clear_active_follows`) is DELETED — those public action strings are
        // retired. The wasm reducer's `open_interest` / `declare_active_follows_feed`
        // methods remain as INTERNAL composition glue (the web app's feed setup
        // drives them directly through the `WasmRuntime` Rust facade, not through a
        // host action string). There is no public wasm `open_feed` doorway yet (the
        // session registry + perspective compiler are native-only — see #1740).
        //
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
        let reason = write_path_unavailable_reason(self.has_active_account());
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action.action_type,
            correlation_id: action.correlation_id,
            reason,
        })])
    }
}
