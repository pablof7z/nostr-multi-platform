//! Action-dispatch arm of [`super::WasmRuntime::handle`].
//!
//! Split out of `runtime.rs` (LOC ceiling) — the binary `dispatch_bytes`
//! doorway and the legacy JSON `dispatch` router plus `accepted_with_snapshot`
//! are a cohesive unit: they translate a host write command into the
//! `KernelReducer` mutation + the `[ActionAccepted, UpdateBytes?]` reply. The
//! relay-driven snapshot push and the `Start`/`Stop`/`SetIdentity` arms stay in
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
use nmp_core::substrate::ActionContext;
use nmp_core::KernelUpdate;

/// The kernel publish namespace. Publishing stays honestly-disabled on the web
/// preview until the composition root wires a real `OutboxResolver` (#1008), so
/// the byte doorway never runs `start_bytes` for it — the typed decode would
/// validate, but the terminal write has nowhere to go. Routing it through the
/// honest write-path reason keeps the host's "not available in this preview"
/// banner accurate.
const PUBLISH_NAMESPACE: &str = "nmp.publish";

use super::{WasmRuntime, WasmRuntimeError};

/// Render an [`ActionRejection`](nmp_core::substrate::ActionRejection) into the
/// host-facing reason string carried by a fail-closed `CapabilityFailure`. The
/// wasm twin of the native FFI `rejection_message` (`nmp-ffi/src/action.rs`):
/// `ActionRejection` is data (no `Display`), so each variant is mapped to its
/// raw prose explicitly. Used for typed-decode / `schema_version` / `start()`
/// rejections surfaced by `start_bytes`.
fn rejection_reason(rejection: nmp_core::substrate::ActionRejection) -> String {
    use nmp_core::substrate::ActionRejection;
    match rejection {
        ActionRejection::Invalid(s) => s,
        ActionRejection::InvalidCoded { message, .. } => message,
        ActionRejection::Unauthorized(s) => format!("unauthorized: {s}"),
        ActionRejection::Conflict(s) => format!("conflict: {s}"),
    }
}

/// Wall-clock milliseconds for the action-id mint inside `start_bytes`.
///
/// The minted id is discarded on the byte lane (the operation identity is the
/// host-supplied `correlation_id`, ADR-0064 §4), so the exact value is
/// irrelevant — but the call must not panic on wasm32. `std::time::SystemTime`
/// traps on wasm32, so the browser path reads `js_sys::Date::now()` (the same
/// clock the relay-pool backoff uses); native reads `SystemTime`.
#[cfg(target_arch = "wasm32")]
fn wall_clock_ms() -> u64 {
    js_sys::Date::now() as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn wall_clock_ms() -> u64 {
    // This fn is `#[cfg(not(target_arch = "wasm32"))]` — never compiled on
    // wasm32 (the wasm32 twin above reads `js_sys::Date::now()`), so the D20
    // panic-on-wasm hazard cannot arise here.
    use std::time::{SystemTime, UNIX_EPOCH}; // doctrine-allow: D20 — native-only branch, cfg-gated off wasm32
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
    /// one doorway.
    ///
    /// ADR-0064 / S3 (#1751) — the typed twin of the native FFI
    /// `crates/nmp-ffi/src/action/bytes.rs::dispatch_action_bytes`: the OPAQUE
    /// per-crate payload is routed into the registry's typed
    /// [`ActionRegistry::start_bytes`](nmp_core::ActionRegistry::start_bytes),
    /// which runs the per-crate `decode_payload` + the fail-closed
    /// `schema_version` gate + the module's `start()` validation BEFORE anything
    /// else. A registered non-publish namespace (NIP-02 follow/unfollow/
    /// follow_many, NIP-25 react/unreact — whatever the composition root wired)
    /// therefore DECODES its typed FlatBuffers payload and reaches `start()`,
    /// instead of returning the generic envelope-level `CapabilityFailure`.
    ///
    /// Two honest boundaries remain, and both surface as data-shaped
    /// `CapabilityFailure` (fail-closed, never a silent accept — D6 / zero-debt):
    ///
    /// * **No signer / unknown namespace / typed-decode or `start()` rejection.**
    ///   No active account → the `signer_not_installed` reason (checked first,
    ///   before the registry is touched). An unknown namespace, a payload that
    ///   fails the `schema_version` gate, or a `start()` rejection → the RAW
    ///   rejection text from `start_bytes` (the module never ran, or rejected).
    /// * **Validated, but the terminal write needs #1008.** Even after the
    ///   typed payload validates, the module's `execute()` enqueues an
    ///   `ActorCommand` (e.g. `Follow`) that the native actor turns into a kind:3
    ///   publish via an `OutboxResolver`. The wasm preview has no actor and no
    ///   real `OutboxResolver` (the kernel default `NoopOutboxResolver` resolves
    ///   zero targets → silent drop), so the terminal write stays
    ///   honestly-disabled behind the same `publish_not_supported_in_web_preview`
    ///   token publish uses. Wiring that resolver is #1008 — the separate
    ///   prerequisite for the web write path actually reaching the wire. We do
    ///   NOT call `execute_bytes` here: doing so with a dropping send-sink would
    ///   ACK an action that never reaches the wire (a silent always-fail the
    ///   zero-debt rule forbids).
    ///
    /// Publishing (`nmp.publish`) skips `start_bytes` entirely — its terminal
    /// write has the same #1008 dependency and there is no extra typed-decode to
    /// exercise here that the kernel `PublishModule` doesn't already gate, so it
    /// short-circuits to the honest write-path reason.
    fn route_decoded_dispatch(&mut self, decoded: DecodedDispatch) -> Vec<WorkerEvent> {
        let DecodedDispatch {
            correlation_id,
            action_namespace,
            payload,
        } = decoded;

        // Fail-closed, checked before the registry: no active account → the user
        // has not signed in, so no write (typed or otherwise) can be attributed.
        if !self.has_active_account() {
            return vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_namespace,
                correlation_id,
                reason: write_path_unavailable_reason(false),
            })];
        }

        // Publishing stays honestly-disabled pending the #1008 OutboxResolver.
        if action_namespace == PUBLISH_NAMESPACE {
            return vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_namespace,
                correlation_id,
                reason: write_path_unavailable_reason(true),
            })];
        }

        // S3 — route the opaque payload into the typed registry doorway. This is
        // the VALIDATION gate: `start_bytes` runs the per-crate `decode_payload`
        // + the fail-closed `schema_version` gate + the module's `start()`. An
        // unknown namespace, a not-typed-capable module, a decode/version trip,
        // or a `start()` rejection all surface as a data-shaped
        // `CapabilityFailure` carrying the RAW reason (the module never ran).
        let now_ms = wall_clock_ms();
        let mut ctx = ActionContext {};
        match self
            .action_registry
            .start_bytes(&mut ctx, now_ms, &action_namespace, &payload)
        {
            Ok(_validated) => {
                // The typed payload decoded and `start()` validated it — the S3
                // doorway crossed. The terminal write (execute → ActorCommand →
                // kind:N publish) still needs the #1008 OutboxResolver/actor the
                // wasm preview does not wire, so the write stays honestly-disabled
                // behind the same publish-not-supported token rather than being
                // ACKed and silently dropped.
                vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: action_namespace,
                    correlation_id,
                    reason: write_path_unavailable_reason(true),
                })]
            }
            Err(rejection) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_namespace,
                correlation_id,
                reason: rejection_reason(rejection),
            })],
        }
    }

    /// Whether the kernel has an active account seeded (via `SetIdentity` /
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
