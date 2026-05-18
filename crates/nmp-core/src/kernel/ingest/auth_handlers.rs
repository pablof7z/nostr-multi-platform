//! NIP-42 AUTH ingest handlers. Extracted from `ingest/mod.rs` to keep the
//! parent module under the AGENTS.md soft cap. See `kernel/auth.rs` for the
//! protocol primitives (parsers + driver FSM); this file is the **kernel-side
//! glue** that drives the driver, dispatches the signer, and reflects state
//! into `RelayHealth`.

use super::super::*;
use crate::subs::RelayAuthState;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// Wire key for the `RelayStatus.auth` field — ADR-0007 §1 / matches the
/// `nmp_nip42::state::RelayAuthState::as_status_key` keys verbatim so the
/// two surfaces stay aligned should the protocol module ever be re-introduced
/// as a kernel dependency.
pub(super) fn auth_state_key(state: &RelayAuthState) -> &'static str {
    match state {
        RelayAuthState::NotRequired => "not_required",
        RelayAuthState::ChallengeReceived => "challenge_received",
        RelayAuthState::Authenticating => "authenticating",
        RelayAuthState::Authenticated => "authenticated",
        RelayAuthState::Failed => "failed",
    }
}

/// Convert lifecycle `WireFrame`s (emitted by AuthGate-on-Authenticated) into
/// the kernel's `OutboundMessage` shape. Frames not addressed to `role.url()`
/// are skipped — the AuthGate stores per-relay so this is belt-and-braces.
pub(super) fn wire_frames_to_outbound(
    frames: Vec<crate::subs::WireFrame>,
    role: RelayRole,
) -> Vec<OutboundMessage> {
    use crate::subs::WireFrame;
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        match frame {
            WireFrame::Req {
                relay_url,
                sub_id,
                filter_json,
                ..
            } if relay_url == role.url() => {
                out.push(OutboundMessage {
                    role,
                    relay_url,
                    text: format!("[\"REQ\",\"{sub_id}\",{filter_json}]"),
                });
            }
            WireFrame::Close { relay_url, sub_id } if relay_url == role.url() => {
                out.push(OutboundMessage {
                    role,
                    relay_url,
                    text: format!("[\"CLOSE\",\"{sub_id}\"]"),
                });
            }
            _ => {}
        }
    }
    out
}

impl Kernel {
    /// M5+M2+M8 wiring: handle an `["AUTH", <challenge>]` frame from a relay.
    ///
    /// Transitions the per-relay `Nip42DriverState` to `ChallengeReceived`,
    /// fans the new state through the lifecycle's `AuthGate`, then (when an
    /// auth-signer is bound) builds and signs the kind:22242 event,
    /// transitioning to `Authenticating` and emitting the
    /// `["AUTH", <signed_event>]` wire frame for outbound.
    ///
    /// Per D8: this method never sets `changed_since_emit = true`. AUTH-state
    /// transitions are diagnostic; only data-event ingestion bumps view rev.
    pub(super) fn handle_auth_challenge(
        &mut self,
        role: RelayRole,
        array: &[Value],
    ) -> Vec<OutboundMessage> {
        use super::super::auth::{build_auth_event, parse_auth_challenge};

        let Some(challenge) = parse_auth_challenge(array) else {
            return Vec::new();
        };

        let driver = self.nip42_drivers.entry(role).or_default();
        driver.on_auth_frame(challenge.clone());

        // Fan ChallengeReceived into the lifecycle AuthGate so subsequent REQs
        // to this relay are buffered. partition() returns no flushed frames on
        // a pause-transition.
        let relay_url = role.url().to_string();
        let _paused = self
            .lifecycle
            .handle_auth_state_change(relay_url.clone(), RelayAuthState::ChallengeReceived);
        self.update_relay_auth_status(role, RelayAuthState::ChallengeReceived, None);

        let Some(signer) = self.auth_signer.clone() else {
            self.log(format!(
                "AUTH challenge from {} but no signer bound — staying in ChallengeReceived",
                role.key()
            ));
            return Vec::new();
        };

        let Some(active_pubkey) = self.auth_signer_pubkey.clone() else {
            self.log(format!(
                "AUTH challenge from {}: signer bound but no active pubkey",
                role.key()
            ));
            return Vec::new();
        };

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let unsigned = build_auth_event(active_pubkey, role.url(), &challenge, created_at);
        match signer(&unsigned) {
            Ok(signed) => {
                // Structural-validation guard against buggy/malicious signers
                // that mutate the kind, drop the challenge tag, or return
                // malformed ids/sigs. Schnorr verification is separately
                // handled at the store boundary; this is the shape gate.
                if let Err(reason) = super::super::auth::validate_signed_for(&signed, &challenge) {
                    self.log(format!(
                        "AUTH signer returned invalid event for {}: {reason}",
                        role.key()
                    ));
                    let driver = self.nip42_drivers.entry(role).or_default();
                    driver.record_signer_failure();
                    let _ = self
                        .lifecycle
                        .handle_auth_state_change(relay_url, RelayAuthState::Failed);
                    self.update_relay_auth_status(role, RelayAuthState::Failed, Some(reason));
                    // T76 fail-closed: discard any REQs already deferred for
                    // this relay so they cannot leak unauthenticated.
                    self.purge_deferred_reqs_for(role);
                    return Vec::new();
                }
                let event_id = signed.id.clone();
                let driver = self.nip42_drivers.entry(role).or_default();
                if !driver.record_dispatch(event_id.clone()) {
                    return Vec::new();
                }
                let _ = self
                    .lifecycle
                    .handle_auth_state_change(relay_url, RelayAuthState::Authenticating);
                self.update_relay_auth_status(role, RelayAuthState::Authenticating, None);
                let wire = json!([
                    "AUTH",
                    {
                        "id": signed.id,
                        "pubkey": signed.unsigned.pubkey,
                        "kind": signed.unsigned.kind,
                        "tags": signed.unsigned.tags,
                        "content": signed.unsigned.content,
                        "created_at": signed.unsigned.created_at,
                        "sig": signed.sig,
                    }
                ])
                .to_string();
                self.log(format!("AUTH dispatched to {} ({event_id})", role.key()));
                vec![OutboundMessage {
                    role,
                    relay_url: role.url().to_string(),
                    text: wire,
                }]
            }
            Err(reason) => {
                self.log(format!("AUTH signer failed for {}: {reason}", role.key()));
                let driver = self.nip42_drivers.entry(role).or_default();
                driver.record_signer_failure();
                let _ = self
                    .lifecycle
                    .handle_auth_state_change(relay_url, RelayAuthState::Failed);
                self.update_relay_auth_status(role, RelayAuthState::Failed, Some(reason));
                self.purge_deferred_reqs_for(role);
                Vec::new()
            }
        }
    }

    /// M5+M2+M8 wiring: handle an `["OK", <event_id>, <accepted>, <reason>]`
    /// frame. Correlates against the per-relay pending kind:22242. On match,
    /// transitions to `Authenticated` (and flushes AuthGate's buffered REQs
    /// back to outbound) or `Failed`. Non-AUTH OKs are no-ops here.
    pub(super) fn handle_auth_ok(
        &mut self,
        role: RelayRole,
        array: &[Value],
    ) -> Vec<OutboundMessage> {
        use super::super::auth::parse_ok_frame;

        let Some(ok) = parse_ok_frame(array) else {
            return Vec::new();
        };
        let driver = self.nip42_drivers.entry(role).or_default();
        let Some(new_state) = driver.on_ok_frame(&ok) else {
            return Vec::new();
        };
        let relay_url = role.url().to_string();
        let flushed = self
            .lifecycle
            .handle_auth_state_change(relay_url, new_state.clone());
        let reason = if matches!(new_state, RelayAuthState::Failed) {
            Some(if ok.reason.is_empty() {
                "relay rejected AUTH".to_string()
            } else {
                format!("relay rejected AUTH: {}", ok.reason)
            })
        } else {
            None
        };
        self.update_relay_auth_status(role, new_state.clone(), reason);
        if matches!(new_state, RelayAuthState::Failed) {
            // T76 fail-closed: relay rejected our AUTH event — discard any
            // deferred REQs for this relay rather than leak them.
            self.purge_deferred_reqs_for(role);
        }
        self.log(format!("AUTH ok from {}: {new_state:?}", role.key()));
        // Flushed WireFrames flow back to outbound. The kernel's hand-rolled
        // `req()` is the M1 path, not the lifecycle, so the AuthGate's pending
        // buffer is empty in the kernel-only execution; the plumbing is in
        // place so when M11 migrates view modules onto `LogicalInterest` the
        // path Just Works.
        wire_frames_to_outbound(flushed, role)
    }

    /// Reflect the per-relay auth state into the diagnostic
    /// `RelayStatus.auth` field. AUTH-state transitions DO bump
    /// `changed_since_emit` so the diagnostic surface (RelayStatus + toast)
    /// re-emits; the actor's ≤60 Hz/view cap (D8) handles throughput. The
    /// `nip42_kernel_auth_does_not_bump_view_rev` test pins the narrower
    /// invariant that AUTH does NOT directly bump `rev` — that's done by
    /// the next `make_update` whose schedule is rate-capped.
    ///
    /// Without this dirty-mark the user could not see a Failed AUTH state
    /// (`docs/plan/m5-nip42.md` §19 explicitly requires visible diagnostic
    /// surfacing of the `Failed` transition).
    pub(super) fn update_relay_auth_status(
        &mut self,
        role: RelayRole,
        state: RelayAuthState,
        reason: Option<String>,
    ) {
        let key = auth_state_key(&state);
        let relay = self.relay_mut(role);
        relay.auth = key.to_string();
        if let Some(r) = reason {
            relay.last_error = Some(r);
        }
        // D8: bump the dirty flag so the diagnostic surface re-emits on the
        // next actor tick. The actor's emit-interval throttle (≤60 Hz/view)
        // bounds throughput; per-tick coalescing handles burst scenarios.
        self.changed_since_emit = true;
    }
}
