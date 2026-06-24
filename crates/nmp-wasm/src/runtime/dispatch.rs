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

use nmp_core::actor::ActorCommand;
use nmp_core::actor::PublishCommand;

use crate::dispatch_routing::{
    execute_ref_dispatch, kernel_action_from_dispatch, ref_dispatch_from_action,
    write_path_unavailable_reason,
};
use crate::protocol::{ActionDispatch, CapabilityFailure, WorkerEvent};
use nmp_core::dispatch_envelope::{decode_dispatch_envelope, DecodedDispatch};
use nmp_core::substrate::ActionContext;
use nmp_core::KernelUpdate;

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
    /// the existing one doorway, carrying the OPAQUE payload verbatim.
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
    /// ADR-0064 / S3 (#1751) / #1008 — the typed twin of the native FFI
    /// `crates/nmp-ffi/src/action/bytes.rs::dispatch_action_bytes`. After
    /// `start_bytes` validates the typed payload and `start()` accepts it,
    /// `execute_bytes` is called with a wasm-aware `send` closure that handles
    /// each `ActorCommand` variant:
    ///
    /// * **`PublishSignedEvent`** — routes directly to the kernel publish engine
    ///   via [`nmp_core::KernelReducer::publish_pre_signed`]. The
    ///   `WasmOutboxResolver` wired at `Start` (#1008) provides the write relay
    ///   set. Returns `ActionAccepted + UpdateBytes`.
    ///
    /// * **`PublishRawEvent` / `PublishProfile`** — needs the `BeginSign`
    ///   capability round-trip. Builds unsigned JSON, parks a sign op via
    ///   [`nmp_core::KernelReducer::begin_sign_roundtrip`], and returns
    ///   `WorkerEvent::SignRequest` for the main-thread broker.
    ///
    /// Fail-closed for no-signer (no active account) and unknown namespace /
    /// decode / `start()` rejection paths.
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

        // S3 — route the opaque payload into the typed registry doorway.
        // `start_bytes` runs per-crate `decode_payload` + fail-closed
        // `schema_version` gate + `start()`. Unknown namespace, not-typed-capable
        // module, decode/version trip, or `start()` rejection surface as
        // data-shaped `CapabilityFailure` (the module never ran).
        let now_ms = wall_clock_ms();
        let mut ctx = ActionContext {};
        match self
            .action_registry
            .start_bytes(&mut ctx, now_ms, &action_namespace, &payload)
        {
            Err(rejection) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: action_namespace,
                correlation_id,
                reason: rejection_reason(rejection),
            })],
            Ok(_action_id) => {
                // Typed payload validated. Now execute: collect ActorCommands
                // and route each through the wasm-aware handler.
                let commands =
                    std::rc::Rc::new(std::cell::RefCell::new(Vec::<ActorCommand>::new()));
                let commands_clone = std::rc::Rc::clone(&commands);
                let exec_result = self.action_registry.execute_bytes(
                    &action_namespace,
                    &payload,
                    &correlation_id,
                    &move |cmd| commands_clone.borrow_mut().push(cmd),
                );
                if let Err(failure) = exec_result {
                    return vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                        capability: action_namespace,
                        correlation_id,
                        reason: failure.message,
                    })];
                }
                let collected = std::rc::Rc::try_unwrap(commands)
                    .expect("no other Rc holders after execute_bytes returns")
                    .into_inner();
                self.execute_actor_commands(collected, &action_namespace, correlation_id)
            }
        }
    }

    /// Route collected [`ActorCommand`]s from a validated typed dispatch to
    /// the wasm-aware publish handlers.
    ///
    /// Called only after `start_bytes` + `execute_bytes` have both succeeded.
    /// Returns `ActionAccepted + UpdateBytes` for synchronous pre-signed paths,
    /// or `SignRequest + UpdateBytes` for the async signing path.
    fn execute_actor_commands(
        &mut self,
        commands: Vec<ActorCommand>,
        action_namespace: &str,
        correlation_id: String,
    ) -> Vec<WorkerEvent> {
        let mut events: Vec<WorkerEvent> = Vec::new();
        for cmd in commands {
            match cmd {
                // Pre-signed event: route through the kernel publish engine.
                // The `WasmOutboxResolver` (#1008) provides the write relay set.
                ActorCommand::Publish(PublishCommand::SignedEvent {
                    raw,
                    target,
                    correlation_id: cid,
                }) => {
                    let outbound = self
                        .reducer
                        .borrow_mut()
                        .publish_pre_signed(raw, target, cid);
                    self.fan_outbound(outbound);
                    self.request_event_drain();
                    events.push(WorkerEvent::ActionAccepted {
                        action_type: action_namespace.to_string(),
                        correlation_id: correlation_id.clone(),
                    });
                    events.push(self.snapshot_event());
                }
                // Unsigned event (arbitrary kind): build the unsigned JSON and
                // start the BeginSign round-trip. The main-thread broker calls
                // `window.nostr.signEvent` and re-enters with
                // `DeliverSignerResponse`; from there `deliver_signer_response`
                // emits `SignCompleted` and the host publishes via a follow-up
                // `PublishSignedEvent` dispatch or by noting the signed JSON.
                ActorCommand::Publish(PublishCommand::RawEvent {
                    kind,
                    tags,
                    content,
                    target: _,
                    signer_pubkey: _,
                    correlation_id: cid,
                }) => {
                    let account_pubkey = match self.reducer.borrow().active_account_pubkey() {
                        Some(pk) => pk,
                        None => {
                            events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                                capability: action_namespace.to_string(),
                                correlation_id: correlation_id.clone(),
                                reason: write_path_unavailable_reason(false),
                            }));
                            continue;
                        }
                    };
                    let created_at = wall_clock_ms() / 1000;
                    let unsigned_json = serde_json::json!({
                        "pubkey": account_pubkey,
                        "kind": kind,
                        "tags": tags,
                        "content": content,
                        "created_at": created_at,
                    })
                    .to_string();
                    match self
                        .reducer
                        .borrow_mut()
                        .begin_sign_roundtrip(account_pubkey, &unsigned_json)
                    {
                        Ok(req) => {
                            self.request_event_drain();
                            events.push(WorkerEvent::SignRequest {
                                correlation_id: req.correlation_id,
                                account_pubkey: req.account_pubkey,
                                unsigned_json: req.unsigned_json,
                            });
                        }
                        Err(reason) => {
                            events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                                capability: action_namespace.to_string(),
                                correlation_id: cid.unwrap_or(correlation_id.clone()),
                                reason,
                            }));
                        }
                    }
                }
                // Profile (kind:0): build the kind:0 content JSON and start the
                // BeginSign round-trip, same as PublishRawEvent above.
                ActorCommand::Publish(PublishCommand::Profile {
                    fields,
                    correlation_id: cid,
                }) => {
                    let account_pubkey = match self.reducer.borrow().active_account_pubkey() {
                        Some(pk) => pk,
                        None => {
                            events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                                capability: action_namespace.to_string(),
                                correlation_id: correlation_id.clone(),
                                reason: write_path_unavailable_reason(false),
                            }));
                            continue;
                        }
                    };
                    let content =
                        serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_string());
                    let created_at = wall_clock_ms() / 1000;
                    let unsigned_json = serde_json::json!({
                        "pubkey": account_pubkey,
                        "kind": 0u32,
                        "tags": serde_json::Value::Array(vec![]),
                        "content": content,
                        "created_at": created_at,
                    })
                    .to_string();
                    match self
                        .reducer
                        .borrow_mut()
                        .begin_sign_roundtrip(account_pubkey, &unsigned_json)
                    {
                        Ok(req) => {
                            self.request_event_drain();
                            events.push(WorkerEvent::SignRequest {
                                correlation_id: req.correlation_id,
                                account_pubkey: req.account_pubkey,
                                unsigned_json: req.unsigned_json,
                            });
                        }
                        Err(reason) => {
                            events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                                capability: action_namespace.to_string(),
                                correlation_id: cid.unwrap_or(correlation_id.clone()),
                                reason,
                            }));
                        }
                    }
                }
                // Any other ActorCommand variant: the wasm runtime has no
                // actor thread to handle it. Surface a CapabilityFailure so
                // the host sees an honest "not handled" signal rather than a
                // silent drop.
                other => {
                    events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                        capability: action_namespace.to_string(),
                        correlation_id: correlation_id.clone(),
                        reason: format!(
                            "wasm_actor_command_unhandled: ActorCommand variant {:?} is not \
                             handled by the wasm runtime — it requires the native actor thread.",
                            std::mem::discriminant(&other)
                        ),
                    }));
                }
            }
        }
        // If no commands were emitted (a module with side-effects only),
        // return a plain ActionAccepted + snapshot.
        if events.is_empty() {
            return self.accepted_with_snapshot(action_namespace.to_string(), correlation_id);
        }
        events
    }

    /// Whether the kernel has an active account seeded (via `SetIdentity` /
    /// `set_active_account`). The two honest write-unavailability states key on
    /// this instead of a persistent signer slot (removed in #1743 Cut A,
    /// ADR-0064 §5): no account → `signer_not_installed`.
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
        self.request_event_drain();
        vec![
            WorkerEvent::ActionAccepted {
                action_type,
                correlation_id,
            },
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
            self.request_event_drain();
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
        // #1740 step 8: the raw feed-verb dispatch arm is DELETED (see
        // dispatch_routing.rs comment for full rationale).
        //
        // Kernel-namespace actions (`nmp.kernel.start`, `open_uri`, etc.) map
        // to `KernelAction` variants and run through `KernelReducer::reduce`.
        if let Some(kernel_action) = kernel_action_from_dispatch(&action) {
            let update = self.reducer.borrow_mut().reduce(kernel_action);
            match update {
                KernelUpdate::Started { .. } => {
                    self.meta.borrow_mut().started = true;
                }
                KernelUpdate::Stopped { .. } => {
                    self.meta.borrow_mut().started = false;
                }
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
