//! Pub(crate) kernel-operation helpers for `BrowserRuntimeHandle` (#2038 item A).
//!
//! These methods expose the narrow set of kernel operations the wasm entry
//! point (`crates/nmp-browser-runtime/src/wasm/`) needs to implement the full
//! `WorkerRequest` protocol without exposing the raw `KernelReducer` directly.
//! They live inside `crate::runtime` so they can reach the `pub(super)` /
//! `pub(crate)` fields on `BrowserRuntime` through `handle.runtime`.
//!
//! # D4 (single-writer)
//!
//! Every mutable method here is `&mut self` on `BrowserRuntimeHandle`, which
//! is the sole writer of `BrowserRuntime` (and therefore `KernelReducer`).
//! They must only be called from the wasm request-handler path, never
//! concurrently with `pump()`.

use std::cell::RefCell;

use nmp_core::substrate::{ActionContext, ActionRejection};
use nmp_core::{
    CommandApplyOutcome, OutboundMessage, RefLiveness, RefNamespace, RefResolveMetadata, RefShape,
    SignRoundTripRequest,
};

use super::handle::BrowserRuntimeHandle;
use super::snapshot::SnapshotOutcome;
use crate::runtime::PendingSignedPublish;

// ── DispatchBytesResult ──────────────────────────────────────────────────────

/// Outcome of [`BrowserRuntimeHandle::apply_dispatch_bytes`].
///
/// Produced by the action-registry routing arm so the wasm layer can map to
/// `WorkerEvent`s without needing direct access to the action registry.
#[derive(Debug)]
pub(crate) enum DispatchBytesResult {
    /// Command applied, outbound fanned, snapshot ready.
    Applied { action_type: String, correlation_id: String },
    /// Command needs an async sign round-trip.
    SignRequired {
        correlation_id: String,
        account_pubkey: String,
        unsigned_json: String,
    },
    /// Typed decode / `start()` / `execute()` rejection or kernel unsupported.
    Rejected { capability: String, correlation_id: String, reason: String },
    /// No active account — fail-closed write gate.
    NoActiveAccount { capability: String, correlation_id: String },
    /// DispatchEnvelope decode failed (bad file id, oversize, etc.).
    DecodeError { message: String },
}

// ── Kernel-op methods on BrowserRuntimeHandle ────────────────────────────────

impl BrowserRuntimeHandle {
    /// Produce the next merged snapshot frame.
    ///
    /// Returns `Some(bytes)` on success (or falls back to the last known-good
    /// frame on `Degraded`). Returns `None` on a terminal `Panic` frame.
    pub(crate) fn produce_snapshot_bytes(&mut self, running: bool) -> Option<Vec<u8>> {
        match self.next_frame(running) {
            SnapshotOutcome::Frame(bytes) => Some(bytes),
            SnapshotOutcome::Degraded { last_good, .. } => last_good,
            SnapshotOutcome::Panic(_) => None,
        }
    }

    /// Seed the kernel's active account from a validated canonical pubkey hex.
    pub(crate) fn apply_set_active_account(
        &mut self,
        canonical_pubkey_hex: String,
    ) -> Vec<OutboundMessage> {
        self.runtime.reducer.set_active_account(canonical_pubkey_hex)
    }

    /// Fan outbound relay frames to the pool's WebSocket drivers.
    pub(crate) fn fan_out_outbound(&mut self, outbound: Vec<OutboundMessage>) {
        self.runtime.relay_pool.fan_out_outbound(&outbound);
    }

    /// Begin a host-brokered sign round-trip (NIP-07 / NIP-46).
    /// Returns the sign request on success or an error reason string.
    pub(crate) fn begin_sign_roundtrip(
        &mut self,
        account_pubkey: String,
        unsigned_json: &str,
    ) -> Result<SignRoundTripRequest, String> {
        self.runtime.reducer.begin_sign_roundtrip_at(
            account_pubkey,
            unsigned_json,
            nmp_core::time::Instant::now(),
        )
    }

    /// Apply a raw-key resolve-ref operation (ADR-0063).
    pub(crate) fn apply_resolve_ref_with_metadata(
        &mut self,
        namespace: RefNamespace,
        key: String,
        consumer_id: String,
        shape: RefShape,
        liveness: RefLiveness,
        metadata: RefResolveMetadata,
    ) -> Vec<OutboundMessage> {
        self.runtime.reducer.resolve_ref_with_metadata_at(
            namespace,
            key,
            consumer_id,
            shape,
            liveness,
            metadata,
            nmp_core::time::Instant::now(),
        )
    }

    /// Apply a raw-key release-ref operation (ADR-0063).
    pub(crate) fn apply_release_ref(
        &mut self,
        namespace: RefNamespace,
        key: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        self.runtime.reducer.release_ref(namespace, key, consumer_id)
    }

    /// JSON snapshot of recent routing decisions (log-safe diagnostics).
    pub(crate) fn recent_routing_decisions_json(&self) -> String {
        self.runtime.reducer.recent_routing_decisions_json()
    }

    /// Whether the kernel has a seeded active account.
    pub(crate) fn active_account_pubkey_inner(&self) -> Option<String> {
        self.runtime.reducer.active_account_pubkey()
    }

    /// Whether any relay is currently connected.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn any_relay_connected_inner(&self) -> bool {
        self.runtime.reducer.any_relay_connected()
    }

    /// Apply a `DispatchEnvelope` byte payload through the action registry and
    /// kernel (ADR-0064 / S3 #1751). Mirrors `nmp-wasm`'s `dispatch_bytes` /
    /// `route_decoded_dispatch` but operates on the owned `BrowserRuntime`
    /// fields (not `Rc<RefCell<>>`) so it is usable from `BrowserRuntimeHandle`
    /// methods.
    ///
    /// D4 (single-writer): only call this from the request-handler path, never
    /// concurrently with `pump()`.
    pub(crate) fn apply_dispatch_bytes(&mut self, bytes: &[u8]) -> DispatchBytesResult {
        use nmp_core::dispatch_envelope::decode_dispatch_envelope;

        let decoded = match decode_dispatch_envelope(bytes) {
            Ok(d) => d,
            Err(err) => {
                return DispatchBytesResult::DecodeError {
                    message: err.to_string(),
                };
            }
        };

        let correlation_id = decoded.correlation_id.clone();
        let action_namespace = decoded.action_namespace.clone();
        let payload = decoded.payload.clone();

        // Fail-closed: no active account → no write (D6 — signer_not_installed).
        if self.active_account_pubkey_inner().is_none() {
            return DispatchBytesResult::NoActiveAccount {
                capability: action_namespace,
                correlation_id,
            };
        }

        // S3 — run the typed decode + fail-closed schema_version gate + start().
        let now_ms = self.runtime.reducer.now_ms();
        let mut ctx = ActionContext {};
        let start_result =
            self.runtime
                .action_registry
                .start_bytes(&mut ctx, now_ms, &action_namespace, &payload);

        if let Err(rejection) = start_result {
            return DispatchBytesResult::Rejected {
                capability: action_namespace,
                correlation_id,
                reason: format_rejection(rejection),
            };
        }

        // Execute: collect ActorCommands synchronously (execute_bytes is sync).
        let collected = RefCell::new(Vec::new());
        let exec_result = self.runtime.action_registry.execute_bytes(
            &action_namespace,
            &payload,
            &correlation_id,
            &|cmd| collected.borrow_mut().push(cmd),
        );
        if let Err(failure) = exec_result {
            return DispatchBytesResult::Rejected {
                capability: action_namespace,
                correlation_id,
                reason: failure.message,
            };
        }

        let cmds = collected.into_inner();
        let mut all_outbound: Vec<OutboundMessage> = Vec::new();

        // Apply each command to the kernel.
        for cmd in cmds {
            match self.runtime.reducer.apply_actor_command(cmd) {
                CommandApplyOutcome::Applied(outbound) => {
                    all_outbound.extend(outbound);
                }
                CommandApplyOutcome::NeedsSign {
                    request,
                    target,
                    action_correlation_id,
                } => {
                    // Park the publish continuation keyed on the sign correlation
                    // id (mirrors nmp-wasm/src/runtime/dispatch.rs).
                    let action_cid =
                        action_correlation_id.unwrap_or_else(|| correlation_id.clone());
                    self.runtime.pending_signed_publishes.insert(
                        request.correlation_id.clone(),
                        PendingSignedPublish {
                            action_correlation_id: Some(action_cid),
                            target,
                        },
                    );
                    // Fan whatever outbound we accumulated before the sign gate.
                    let prev_out = std::mem::take(&mut all_outbound);
                    self.fan_out_outbound(prev_out);
                    return DispatchBytesResult::SignRequired {
                        correlation_id: request.correlation_id,
                        account_pubkey: request.account_pubkey,
                        unsigned_json: request.unsigned_json,
                    };
                }
                CommandApplyOutcome::Unsupported { reason } => {
                    return DispatchBytesResult::Rejected {
                        capability: action_namespace,
                        correlation_id,
                        reason,
                    };
                }
            }
        }

        // All commands applied. Fan outbound.
        self.fan_out_outbound(all_outbound);
        DispatchBytesResult::Applied {
            action_type: action_namespace,
            correlation_id,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn format_rejection(rejection: ActionRejection) -> String {
    match rejection {
        ActionRejection::Invalid(s) => s,
        ActionRejection::InvalidCoded { message, .. } => message,
        ActionRejection::Unauthorized(s) => format!("unauthorized: {s}"),
        ActionRejection::Conflict(s) => format!("conflict: {s}"),
    }
}
