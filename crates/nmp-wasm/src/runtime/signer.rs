//! Active-identity seeding + the #1753 S6 wasm signing capability round-trip
//! arms for [`super::WasmRuntime`].

use crate::protocol::{
    BeginSign, CapabilityFailure, DeliverSignerResponse, IdentityRelayPermission,
    RelayBootstrapEntry, SetIdentity, WorkerEvent,
};
use crate::signer_slot;

use super::WasmRuntime;

const MAX_IDENTITY_RELAYS: usize = 32;

fn signed_json_to_raw_event(signed_json: &str) -> Result<nmp_store::RawEvent, String> {
    serde_json::from_str(signed_json)
        .map_err(|e| format!("signed event JSON did not decode as RawEvent: {e}"))
}

#[derive(Default)]
struct RoleFlags {
    read: bool,
    write: bool,
    indexer: bool,
}

fn role_flags(role: &str) -> RoleFlags {
    let mut flags = RoleFlags::default();
    for token in role.split(',').map(|part| part.trim().to_ascii_lowercase()) {
        match token.as_str() {
            "read" => flags.read = true,
            "write" => flags.write = true,
            "both" => {
                flags.read = true;
                flags.write = true;
            }
            "indexer" => flags.indexer = true,
            // Legacy/bootstrap transport token. Treat it as read+write for
            // merge purposes so a signer relay does not erase content reach.
            "content" => {
                flags.read = true;
                flags.write = true;
            }
            _ => {}
        }
    }
    flags
}

fn role_from_flags(flags: RoleFlags) -> Option<String> {
    let mut parts = Vec::new();
    match (flags.read, flags.write) {
        (true, true) => parts.push("both"),
        (true, false) => parts.push("read"),
        (false, true) => parts.push("write"),
        (false, false) => {}
    }
    if flags.indexer {
        parts.push("indexer");
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

fn merge_roles(existing: &str, incoming: &str) -> String {
    let mut flags = role_flags(existing);
    let add = role_flags(incoming);
    flags.read |= add.read;
    flags.write |= add.write;
    flags.indexer |= add.indexer;
    role_from_flags(flags).unwrap_or_else(|| existing.to_string())
}

fn role_for_identity_relay(relay: &IdentityRelayPermission) -> Option<&'static str> {
    match (relay.read, relay.write) {
        (true, true) => Some("both,indexer"),
        (true, false) => Some("read,indexer"),
        // A write relay is where this account's already-published events are
        // likely to live, so it must be a bootstrap read candidate too.
        (false, true) => Some("both,indexer"),
        (false, false) => None,
    }
}

fn identity_relay_entry(relay: &IdentityRelayPermission) -> Option<RelayBootstrapEntry> {
    let role = role_for_identity_relay(relay)?;
    let url = nmp_core::canonical_relay_url(&relay.url)?.to_string();
    Some(RelayBootstrapEntry {
        url,
        role: role.to_string(),
    })
}

impl WasmRuntime {
    fn merge_identity_relays(&mut self, relays: &[IdentityRelayPermission]) -> bool {
        if relays.is_empty() {
            return false;
        }

        let mut merged = self.meta.borrow().relay_bootstrap.clone();
        let mut changed = false;
        for relay in relays.iter().take(MAX_IDENTITY_RELAYS) {
            let Some(entry) = identity_relay_entry(relay) else {
                continue;
            };
            match merged.iter_mut().find(|existing| {
                nmp_core::canonical_relay_url(&existing.url)
                    .map(|url| url.to_string())
                    .as_deref()
                    == Some(entry.url.as_str())
            }) {
                Some(existing) => {
                    let role = merge_roles(&existing.role, &entry.role);
                    if role != existing.role || existing.url != entry.url {
                        existing.url = entry.url;
                        existing.role = role;
                        changed = true;
                    }
                }
                None => {
                    merged.push(entry);
                    changed = true;
                }
            }
        }

        if !changed {
            return false;
        }

        self.reducer.borrow_mut().set_configured_relays(
            merged
                .iter()
                .map(|entry| (entry.url.clone(), entry.role.clone()))
                .collect(),
        );
        self.meta.borrow_mut().relay_bootstrap = merged;
        true
    }

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
                let relay_config_changed = self.merge_identity_relays(&request.identity_relays);
                let outbound = self
                    .reducer
                    .borrow_mut()
                    .set_active_account(canonical_pubkey);
                self.fan_outbound(outbound);
                if relay_config_changed {
                    self.request_event_drain();
                }
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
        match self.reducer.borrow_mut().begin_sign_roundtrip_at(
            request.account_pubkey,
            &request.unsigned_json,
            nmp_core::time::Instant::now(),
        ) {
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
        let now = nmp_core::time::Instant::now();
        let outcome = {
            let mut reducer = self.reducer.borrow_mut();
            match (response.signed_json, response.error) {
                // A broker-reported failure (user rejected / no window.nostr).
                (_, Some(error)) => {
                    reducer.fail_sign_roundtrip_at(&response.correlation_id, &error, now)
                }
                // A signed event to deliver.
                (Some(signed_json), None) => {
                    reducer.deliver_signed_response_at(&response.correlation_id, &signed_json, now)
                }
                // Neither field set — an honest protocol error, failed closed.
                (None, None) => reducer.fail_sign_roundtrip_at(
                    &response.correlation_id,
                    "deliver_signer_response carried neither signed_json nor error",
                    now,
                ),
            }
        };
        match outcome {
            SignRoundTripOutcome::Completed {
                correlation_id,
                signed_json,
            } => {
                let mut events = vec![WorkerEvent::SignCompleted {
                    correlation_id: correlation_id.clone(),
                    signed_json: signed_json.clone(),
                }];
                if let Some(pending) = self.pending_signed_publishes.remove(&correlation_id) {
                    match signed_json_to_raw_event(&signed_json) {
                        Ok(raw) => {
                            let outbound = self.reducer.borrow_mut().publish_pre_signed(
                                raw,
                                pending.target,
                                Some(pending.action_correlation_id.clone()),
                            );
                            self.fan_outbound(outbound);
                            self.request_event_drain();
                            events.push(WorkerEvent::ActionAccepted {
                                action_type: pending.action_namespace,
                                correlation_id: pending.action_correlation_id,
                            });
                            events.push(self.snapshot_event());
                        }
                        Err(reason) => {
                            events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                                capability: pending.action_namespace,
                                correlation_id: pending.action_correlation_id,
                                reason,
                            }));
                        }
                    }
                }
                events
            }
            SignRoundTripOutcome::Failed {
                correlation_id,
                reason,
            } => {
                let mut events = vec![WorkerEvent::SignFailed {
                    correlation_id: correlation_id.clone(),
                    reason: reason.clone(),
                }];
                if let Some(pending) = self.pending_signed_publishes.remove(&correlation_id) {
                    events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                        capability: pending.action_namespace,
                        correlation_id: pending.action_correlation_id,
                        reason,
                    }));
                }
                events
            }
            SignRoundTripOutcome::Unknown { correlation_id } => vec![WorkerEvent::SignFailed {
                correlation_id,
                reason: "no parked sign round-trip matched this correlation id (stale or \
                         duplicate delivery)"
                    .to_string(),
            }],
        }
    }
}
