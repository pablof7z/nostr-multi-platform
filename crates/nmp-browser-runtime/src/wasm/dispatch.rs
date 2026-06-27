// Dispatch handlers are primary consumers for the wasm32 target;
// on native they are exercised only from `#[cfg(test)]` blocks in `core.rs`.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

//! Per-`WorkerRequest` dispatch handlers for `NmpRuntimeCore` (#2038 item A).
//!
//! Split out of `wasm/core.rs` to keep both files under the 500-LOC ceiling
//! (AGENTS.md). `core.rs` owns construction / lifecycle / snapshot plumbing;
//! this file owns the routing of each of the nine `WorkerRequest` variants to
//! the `BrowserRuntimeHandle` kernel-op helpers. The methods are declared in a
//! sibling `impl NmpRuntimeCore` block so they reach the same `pub(super)`
//! fields (`handle`) without changing visibility. No behavior change vs. the
//! pre-split single file.

use crate::runtime::DispatchBytesResult;
use crate::runtime::{RelayConfigAction as RuntimeRelayConfigAction, RelayConfigResult};
use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::core::NmpRuntimeCore;
use super::identity::install_identity;
use super::protocol::{
    relay_bootstrap_from_config, BeginSign, ClientHello, DeliverSignerResponse,
    IdentityRelayPermission, PublishRelayPreferences, RelayConfig, ReleaseRef, ResolveRef,
    RuntimeStatus, SetIdentity, StartConfig, WorkerEvent, WorkerRequest, PROTOCOL_VERSION,
};
use super::ref_routing::{
    invalid_ref_request_reason, ref_dispatch_from_release, ref_dispatch_from_resolve,
    signer_not_installed_reason, RefDispatch,
};

/// Maximum number of identity-provided relays merged into the configured list.
const MAX_IDENTITY_RELAYS: usize = 30;

impl NmpRuntimeCore {
    pub(super) fn dispatch_request(&mut self, request: WorkerRequest) -> Vec<WorkerEvent> {
        match request {
            WorkerRequest::Hello(hello) => self.handle_hello(hello),
            WorkerRequest::Start(config) => self.handle_start(config),
            WorkerRequest::SetIdentity(req) => self.handle_set_identity(req),
            WorkerRequest::ResolveRef(req) => self.handle_resolve_ref(req),
            WorkerRequest::ReleaseRef(req) => self.handle_release_ref(req),
            WorkerRequest::BeginSign(req) => self.handle_begin_sign(req),
            WorkerRequest::DeliverSignerResponse(resp) => self.handle_deliver_signer_response(resp),
            WorkerRequest::DispatchBytes(payload) => self.dispatch_dispatch_bytes(&payload.bytes),
            WorkerRequest::SearchOpen(req) => self.handle_search_open(req),
            WorkerRequest::SearchClose(req) => self.handle_search_close(req),
            WorkerRequest::GroupDiscoveryOpen(req) => self.handle_group_discovery_open(req),
            WorkerRequest::GroupDiscoveryClose(req) => self.handle_group_discovery_close(req),
            WorkerRequest::GroupTimelineOpen(req) => self.handle_group_timeline_open(req),
            WorkerRequest::GroupTimelineClose(req) => self.handle_group_timeline_close(req),
            WorkerRequest::NotificationsOpen(req) => self.handle_notifications_open(req),
            WorkerRequest::NotificationsClose(req) => self.handle_notifications_close(req),
            WorkerRequest::NotificationsMarkRead(req) => self.handle_notifications_mark_read(req),
            WorkerRequest::RelayConfig(req) => self.handle_relay_config(req),
            WorkerRequest::PublishRelayPreferences(req) => {
                self.handle_publish_relay_preferences(req)
            }
            WorkerRequest::CapabilityResult(r) => {
                // No native capability handler in this crate (requires native
                // actor); surface honestly rather than silently dropping.
                vec![WorkerEvent::CapabilityFailure {
                    capability: r.capability,
                    correlation_id: r.correlation_id,
                    reason: "browser_actor_driver_missing: capability completions require \
                             the native actor"
                        .to_string(),
                }]
            }
            WorkerRequest::Stop { correlation_id } => {
                self.handle = None;
                vec![WorkerEvent::RuntimeStatus {
                    status: RuntimeStatus::Stopped,
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_hello(&self, hello: ClientHello) -> Vec<WorkerEvent> {
        if hello.protocol_version != PROTOCOL_VERSION {
            return vec![WorkerEvent::Error {
                code: "protocol_mismatch".to_string(),
                message: format!(
                    "expected protocol version {PROTOCOL_VERSION}, got {}",
                    hello.protocol_version
                ),
                correlation_id: None,
            }];
        }
        vec![WorkerEvent::HelloAccepted {
            protocol_version: PROTOCOL_VERSION,
            status: RuntimeStatus::Ready,
        }]
    }

    fn handle_start(&mut self, config: StartConfig) -> Vec<WorkerEvent> {
        if config.app_id.trim().is_empty() {
            return vec![WorkerEvent::Error {
                code: "invalid_config".to_string(),
                message: "app_id is required".to_string(),
                correlation_id: Some(config.correlation_id),
            }];
        }

        let bootstrap = relay_bootstrap_from_config(config.relays.clone(), config.relay_bootstrap);

        // Build the typed BrowserRuntimeHandle through the builder typestate.
        //
        // Storage gate (#1007 PR-7): if the async pre-`Start` hook
        // (`NmpWasmRuntime::prepare_store`) already opened a durable OPFS-SQLite
        // store and parked it on the core, inject it. Otherwise fall back to the
        // explicit in-memory store. Both arms land on `BrowserAppBuilder<StorageSet>`.
        let storage = BrowserAppBuilder::new();
        let builder = match self.injected_store.take() {
            Some(store) => storage.inject_store(store),
            None => storage.in_memory(),
        }
        .consume_all_builtin_projections();
        nmp_nip50::register_search_scopes(&builder);
        nmp_nip50::register_input_scopes(&builder);

        // Degraded-open diagnostic (#1007 PR-8): if `prepare_store` classified an
        // OPFS open failure and parked the stable reason, thread it onto the
        // kernel so the in-memory fallback session reports the SAME Tier-3
        // `store_open_failure` snapshot the native LMDB degraded-open path emits.
        // `None` (healthy open / native) clears nothing — it is the no-failure case.
        let builder = builder.with_store_open_failure(self.store_open_failure.take());

        let builder = if bootstrap.is_empty() {
            builder.without_initial_relays()
        } else {
            let relay_pairs: Vec<(String, String)> = bootstrap
                .iter()
                .map(|e| (e.url.clone(), e.role.clone()))
                .collect();
            builder.set_relays(relay_pairs)
        };

        let handle = builder
            .decide_providers(BrowserRunConfig {
                app_id: config.app_id,
            })
            .with_system_clock()
            .start();

        self.handle = Some(handle);

        vec![WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Running,
            correlation_id: Some(config.correlation_id),
        }]
    }

    fn handle_set_identity(&mut self, mut req: SetIdentity) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        match install_identity(handle, &mut req) {
            Ok(canonical_hex) => {
                // Merge identity-provided relays BEFORE seeding the active account
                // (#2139 HIGH 4: restores nmp-wasm signer.rs:151 behaviour).
                if !req.identity_relays.is_empty() {
                    let rows = identity_relays_to_rows(&req.identity_relays);
                    if !rows.is_empty() {
                        handle.apply_identity_relays(rows);
                    }
                }
                let outbound = handle.apply_set_active_account(canonical_hex);
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.set_identity".to_string(),
                    correlation_id: req.correlation_id,
                }]
            }
            Err(err) => {
                vec![WorkerEvent::CapabilityFailure {
                    capability: "nmp.set_identity".to_string(),
                    correlation_id: req.correlation_id,
                    reason: err,
                }]
            }
        }
    }

    fn handle_resolve_ref(&mut self, req: ResolveRef) -> Vec<WorkerEvent> {
        let correlation_id = req.correlation_id.clone();
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(correlation_id));
        };

        match ref_dispatch_from_resolve(&req) {
            None => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.kernel.resolve_ref".to_string(),
                correlation_id,
                reason: invalid_ref_request_reason("nmp.kernel.resolve_ref"),
            }],
            Some(RefDispatch::Resolve {
                namespace,
                key,
                consumer_id,
                shape,
                liveness,
                metadata,
            }) => {
                let outbound = handle.apply_resolve_ref_with_metadata(
                    namespace,
                    key,
                    consumer_id,
                    shape,
                    liveness,
                    metadata,
                );
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.kernel.resolve_ref".to_string(),
                    correlation_id,
                }]
            }
            Some(_) => {
                // D6 — total/honest: ref_dispatch_from_resolve can only return
                // Resolve variants; a Release here is an invariant violation.
                // Never panic across the FFI boundary — surface as an error.
                vec![WorkerEvent::Error {
                    code: "invariant_violated".to_string(),
                    message: "ref_dispatch_from_resolve returned unexpected Release variant"
                        .to_string(),
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_release_ref(&mut self, req: ReleaseRef) -> Vec<WorkerEvent> {
        let correlation_id = req.correlation_id.clone();
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(correlation_id));
        };

        match ref_dispatch_from_release(&req) {
            None => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.kernel.release_ref".to_string(),
                correlation_id,
                reason: invalid_ref_request_reason("nmp.kernel.release_ref"),
            }],
            Some(RefDispatch::Release {
                namespace,
                key,
                consumer_id,
            }) => {
                let outbound = handle.apply_release_ref(namespace, &key, &consumer_id);
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.kernel.release_ref".to_string(),
                    correlation_id,
                }]
            }
            Some(_) => {
                // D6 — total/honest: ref_dispatch_from_release can only return
                // Release variants; a Resolve here is an invariant violation.
                // Never panic across the FFI boundary — surface as an error.
                vec![WorkerEvent::Error {
                    code: "invariant_violated".to_string(),
                    message: "ref_dispatch_from_release returned unexpected Resolve variant"
                        .to_string(),
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_relay_config(&mut self, req: RelayConfig) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        let action = match req.action {
            super::protocol::RelayConfigAction::Add => RuntimeRelayConfigAction::Add,
            super::protocol::RelayConfigAction::Remove => RuntimeRelayConfigAction::Remove,
        };
        match handle.apply_relay_config(action, req.url, req.role, &req.correlation_id) {
            RelayConfigResult::Applied {
                action_type,
                correlation_id,
            } => vec![WorkerEvent::ActionAccepted {
                action_type,
                correlation_id,
            }],
            RelayConfigResult::Rejected {
                capability,
                correlation_id,
                reason,
            } => vec![WorkerEvent::CapabilityFailure {
                capability,
                correlation_id,
                reason,
            }],
        }
    }

    fn handle_publish_relay_preferences(
        &mut self,
        req: PublishRelayPreferences,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        dispatch_result_events(handle.publish_relay_preferences(&req.correlation_id))
    }

    fn handle_begin_sign(&mut self, req: BeginSign) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(None);
        };

        match handle.begin_sign_roundtrip(req.account_pubkey, &req.unsigned_json) {
            Ok(sign_req) => vec![WorkerEvent::SignRequest {
                correlation_id: sign_req.correlation_id,
                action_correlation_id: None,
                account_pubkey: sign_req.account_pubkey,
                unsigned_json: sign_req.unsigned_json,
            }],
            Err(reason) => vec![WorkerEvent::SignFailed {
                correlation_id: String::new(),
                reason,
            }],
        }
    }

    fn handle_deliver_signer_response(&mut self, resp: DeliverSignerResponse) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(resp.correlation_id));
        };

        let result = match (resp.signed_json, resp.error) {
            (_, Some(err)) => Err(err),
            (Some(json), None) => Ok(json),
            (None, None) => {
                Err("deliver_signer_response carried neither signed_json nor error".to_string())
            }
        };

        // Enqueue the completion and fire the wake (#2139 BLOCKER 2 fix step 1).
        // NLL ends the `handle` borrow at this statement's semicolon, which allows
        // the `self.pump_once()` call below to take `&mut self` without conflict.
        handle.deliver_signer_response(resp.correlation_id.clone(), result);

        // Pump synchronously to drain the completion channel and collect the
        // sign terminal events (sign_completed / sign_failed) so they return to
        // the JS host from THIS handle_json call rather than being deferred to
        // the async wake timer and subsequently discarded (#2139 BLOCKER 2).
        self.pump_once()
    }

    pub(super) fn dispatch_dispatch_bytes(&mut self, bytes: &[u8]) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(None);
        };

        match handle.apply_dispatch_bytes(bytes) {
            DispatchBytesResult::Applied {
                action_type,
                correlation_id,
            } => {
                vec![WorkerEvent::ActionAccepted {
                    action_type,
                    correlation_id,
                }]
            }
            DispatchBytesResult::SignRequired {
                action_correlation_id,
                correlation_id,
                account_pubkey,
                unsigned_json,
            } => {
                vec![WorkerEvent::SignRequest {
                    correlation_id,
                    action_correlation_id: Some(action_correlation_id),
                    account_pubkey,
                    unsigned_json,
                }]
            }
            DispatchBytesResult::Rejected {
                capability,
                correlation_id,
                reason,
            } => {
                vec![WorkerEvent::CapabilityFailure {
                    capability,
                    correlation_id,
                    reason,
                }]
            }
            DispatchBytesResult::NoActiveAccount {
                capability,
                correlation_id,
            } => {
                vec![WorkerEvent::CapabilityFailure {
                    capability,
                    correlation_id,
                    reason: signer_not_installed_reason(),
                }]
            }
            DispatchBytesResult::DecodeError { message } => {
                vec![WorkerEvent::Error {
                    code: "dispatch_envelope_rejected".to_string(),
                    message,
                    correlation_id: None,
                }]
            }
        }
    }
}

fn dispatch_result_events(result: DispatchBytesResult) -> Vec<WorkerEvent> {
    match result {
        DispatchBytesResult::Applied {
            action_type,
            correlation_id,
        } => vec![WorkerEvent::ActionAccepted {
            action_type,
            correlation_id,
        }],
        DispatchBytesResult::SignRequired {
            action_correlation_id,
            correlation_id,
            account_pubkey,
            unsigned_json,
        } => vec![WorkerEvent::SignRequest {
            correlation_id,
            action_correlation_id: Some(action_correlation_id),
            account_pubkey,
            unsigned_json,
        }],
        DispatchBytesResult::Rejected {
            capability,
            correlation_id,
            reason,
        } => vec![WorkerEvent::CapabilityFailure {
            capability,
            correlation_id,
            reason,
        }],
        DispatchBytesResult::NoActiveAccount {
            capability,
            correlation_id,
        } => vec![WorkerEvent::CapabilityFailure {
            capability,
            correlation_id,
            reason: signer_not_installed_reason(),
        }],
        DispatchBytesResult::DecodeError { message } => vec![WorkerEvent::Error {
            code: "dispatch_envelope_rejected".to_string(),
            message,
            correlation_id: None,
        }],
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(super) fn not_started_error(correlation_id: Option<String>) -> Vec<WorkerEvent> {
    vec![WorkerEvent::Error {
        code: "not_started".to_string(),
        message: "runtime not started — send WorkerRequest::Start first".to_string(),
        correlation_id,
    }]
}

/// Map identity relay permissions to canonical `(url, role)` pairs for merging
/// into the kernel's configured relay list (#2139 HIGH 4).
///
/// Mirrors `nmp-wasm/src/runtime/signer.rs`'s `identity_relay_entry` and
/// `role_for_identity_relay`. Skips entries with no read/write permissions and
/// skips non-canonical URLs.
fn identity_relays_to_rows(relays: &[IdentityRelayPermission]) -> Vec<(String, String)> {
    relays
        .iter()
        .take(MAX_IDENTITY_RELAYS)
        .filter_map(|r| {
            // A write relay is where this account's events are likely published,
            // so it must be a bootstrap read candidate too (mirrors nmp-wasm).
            let role = match (r.read, r.write) {
                (true, true) | (false, true) => "both,indexer",
                (true, false) => "read,indexer",
                (false, false) => return None,
            };
            let url = nmp_core::canonical_relay_url(&r.url)?.to_string();
            Some((url, role.to_string()))
        })
        .collect()
}
