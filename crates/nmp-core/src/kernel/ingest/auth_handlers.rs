//! NIP-42 AUTH ingest handlers. Extracted from `ingest/mod.rs` to keep the
//! parent module under the AGENTS.md soft cap. See `kernel/auth.rs` for the
//! protocol primitives (parsers + driver FSM); this file is the **kernel-side
//! glue** that drives the driver, dispatches the signer, and reflects state
//! into `RelayHealth`.

use super::super::{Arc, Kernel, OutboundMessage, RelayRole};
use crate::subs::RelayAuthState;
use serde_json::{json, Value};
use crate::time::UNIX_EPOCH;

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
/// the kernel's `OutboundMessage` shape. Every flushed frame already carries
/// the correct per-URL relay target (the `AuthGate`'s pending buffer is keyed
/// by `RelayUrl` — see `subs/auth_gate.rs`). The translation is therefore a
/// straight pass-through with `role` stamped for the diagnostic lane.
///
/// **T148**: pre-fix this dropped any frame whose `relay_url != role.url()`
/// (the bootstrap host), which silently discarded every flushed REQ
/// targeting a NIP-65 resolved relay. Post-T148 we trust the `AuthGate`'s
/// per-URL bookkeeping and forward every frame as-is.
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
            } => {
                out.push(OutboundMessage {
                    role,
                    relay_url,
                    text: format!("[\"REQ\",\"{sub_id}\",{filter_json}]"),
                });
            }
            WireFrame::Close { relay_url, sub_id } => {
                out.push(OutboundMessage {
                    role,
                    relay_url,
                    text: format!("[\"CLOSE\",\"{sub_id}\"]"),
                });
            }
        }
    }
    out
}

impl Kernel {
    /// M5+M2+M8 wiring: handle an `["AUTH", <challenge>]` frame from a relay.
    ///
    /// Transitions the per-relay `AuthDriverState` to `ChallengeReceived`,
    /// fans the new state through the lifecycle's `AuthGate`, then (when an
    /// auth-signer is bound) builds and signs the kind:22242 event,
    /// transitioning to `Authenticating` and emitting the
    /// `["AUTH", <signed_event>]` wire frame for outbound.
    ///
    /// `delivering_relay_url` is the URL of the socket the AUTH challenge
    /// arrived on (threaded from `handle_message`/`handle_text`). Per NIP-42
    /// the kind:22242 event's `["relay", <url>]` tag MUST be the URL of the
    /// relay that issued the challenge — this is the replay-protection
    /// binding. The outbound frame's `relay_url` field also targets this URL
    /// so the URL-keyed transport pool (T105 / `fada22b`) routes the AUTH
    /// response back to the same socket. **T125**: pre-fix both stamped
    /// `role.url()` (the lane's bootstrap host), which is wrong for any
    /// relay other than the bootstrap.
    ///
    /// Per D8: this method never sets `changed_since_emit = true`. AUTH-state
    /// transitions are diagnostic; only data-event ingestion bumps view rev.
    pub(super) fn handle_auth_challenge(
        &mut self,
        role: RelayRole,
        delivering_relay_url: &str,
        array: &[Value],
    ) -> Vec<OutboundMessage> {
        use super::super::auth::{build_auth_event, parse_auth_challenge};

        let Some(challenge) = parse_auth_challenge(array) else {
            return Vec::new();
        };

        let driver = self.auth_drivers.entry(role).or_default();
        driver.on_auth_frame(challenge.clone());

        // T148: fan `ChallengeReceived` into the lifecycle's per-URL AuthGate
        // using the DELIVERING relay URL, not the lane's bootstrap host. Pre-
        // T148 this stamped `role.url()` which mis-keyed the AuthGate's pending
        // buffer and the post-Authenticated flush never targeted the right
        // socket. The kernel-side `auth_drivers` map is still per-role (one
        // socket per lane today; per-URL split is a separate, larger change).
        let relay_url = delivering_relay_url.to_string();
        let _paused = self
            .lifecycle
            .handle_auth_state_change(relay_url.clone(), RelayAuthState::ChallengeReceived);
        self.update_relay_auth_status(role, RelayAuthState::ChallengeReceived, None);
        self.sync_transport_from_lane(role, delivering_relay_url);

        // Resolve the signing account for this lane. Two disjoint bindings
        // (kept disjoint by `bind_auth_signer` / `bind_auth_remote`):
        //   * a synchronous local-key `AuthSignerFn` → sign inline below;
        //   * a remote-signer AUTH pubkey (NIP-46 / NIP-55) → there is no
        //     synchronous signer, so build the unsigned event and PARK it for
        //     the async signer port (V-06 / #960). The relay stays
        //     `ChallengeReceived` until the signed frame resolves.
        let resolved = if let Some(c) = self.auth_signers.get(&role) {
            Some((Some(Arc::clone(&c.signer)), c.pubkey_hex.clone()))
        } else {
            self.auth_remote_pubkeys
                .get(&role)
                .map(|pk| (None, pk.clone()))
        };
        let Some((sync_signer, active_pubkey)) = resolved else {
            self.log(format!(
                "AUTH challenge from {} but no signer bound for this role — staying in ChallengeReceived",
                role.key()
            ));
            return Vec::new();
        };

        // Clock seam (kernel/clock.rs): the AUTH event's `created_at` is
        // reducer output, so it reads the injected `Clock` rather than
        // `SystemTime::now()` directly — deterministic-replay requirement.
        let created_at = self
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // T125: stamp the delivering relay's URL on the kind:22242 `relay` tag,
        // not the lane's bootstrap URL. NIP-42 binds the AUTH event to the URL
        // of the relay that issued the challenge (replay protection).
        let unsigned =
            build_auth_event(active_pubkey, delivering_relay_url, &challenge, created_at);

        let Some(sync_signer) = sync_signer else {
            // Remote signer: park the unsigned AUTH for the async signer port.
            // The actor drains `take_pending_auth_signs()` after this frame,
            // routes it through the same `sign_*_nonblocking` seam as every
            // other write, and re-enters `dispatch_signed_auth` on resolution.
            self.log(format!(
                "AUTH challenge from {} — remote signer, parking kind:22242 for the async signer port",
                role.key()
            ));
            self.pending_auth_signs.push(super::super::PendingAuthSign {
                role,
                relay_url: delivering_relay_url.to_string(),
                unsigned,
                challenge,
            });
            return Vec::new();
        };

        // Local key: resolve inline (preserves the synchronous fast path).
        match sync_signer(&unsigned) {
            Ok(signed) => self.dispatch_signed_auth(role, delivering_relay_url, &challenge, signed),
            Err(reason) => {
                self.fail_auth_sign(role, delivering_relay_url, reason);
                Vec::new()
            }
        }
    }

    /// V-06 / #960 — re-entry point for a resolved AUTH signature (local inline
    /// OR the async signer-port round-trip). Runs the structural validation
    /// gate, drives the driver + per-URL `AuthGate` to `Authenticating`, and
    /// emits the CLIENT-`["AUTH", <signed>]` frame routed back to the delivering
    /// socket. On a structurally-invalid signature it fails closed (same as the
    /// inline path) and returns no frames.
    pub(crate) fn dispatch_signed_auth(
        &mut self,
        role: RelayRole,
        delivering_relay_url: &str,
        challenge: &str,
        signed: crate::substrate::SignedEvent,
    ) -> Vec<OutboundMessage> {
        let relay_url = delivering_relay_url.to_string();
        // Structural-validation guard against buggy/malicious signers that
        // mutate the kind, drop the challenge tag, or return malformed ids/sigs.
        // Schnorr verification is separately handled at the store boundary; this
        // is the shape gate.
        if let Err(reason) = super::super::auth::validate_signed_for(&signed, challenge) {
            self.log(format!(
                "AUTH signer returned invalid event for {}: {reason}",
                role.key()
            ));
            self.fail_auth_sign(role, delivering_relay_url, reason);
            return Vec::new();
        }
        let event_id = signed.id.clone();
        let driver = self.auth_drivers.entry(role).or_default();
        if !driver.record_dispatch(event_id.clone()) {
            // No challenge pending (raced a disconnect) — drop silently.
            return Vec::new();
        }
        let _ = self
            .lifecycle
            .handle_auth_state_change(relay_url, RelayAuthState::Authenticating);
        self.update_relay_auth_status(role, RelayAuthState::Authenticating, None);
        self.sync_transport_from_lane(role, delivering_relay_url);
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
        self.log(format!(
            "AUTH dispatched to {} via {} ({event_id})",
            role.key(),
            delivering_relay_url
        ));
        // T125: route the AUTH response to the delivering socket. The URL-keyed
        // transport pool (T105 / fada22b) dispatches by `relay_url`.
        vec![OutboundMessage {
            role,
            relay_url: delivering_relay_url.to_string(),
            text: wire,
        }]
    }

    /// V-06 / #960 — fail an AUTH sign (the signer rejected, threw, timed out,
    /// or returned a structurally-invalid event). Drives the driver + gate to
    /// `Failed`, surfaces the reason on the diagnostic surface, and fails closed
    /// by purging any REQs deferred behind this relay's AUTH gate (T76).
    pub(crate) fn fail_auth_sign(
        &mut self,
        role: RelayRole,
        delivering_relay_url: &str,
        reason: String,
    ) {
        self.log(format!("AUTH signer failed for {}: {reason}", role.key()));
        let driver = self.auth_drivers.entry(role).or_default();
        driver.record_signer_failure();
        let _ = self
            .lifecycle
            .handle_auth_state_change(delivering_relay_url.to_string(), RelayAuthState::Failed);
        self.update_relay_auth_status(role, RelayAuthState::Failed, Some(reason));
        self.sync_transport_from_lane(role, delivering_relay_url);
        self.purge_deferred_reqs_for(role);
    }

    /// M5+M2+M8 wiring: handle an `["OK", <event_id>, <accepted>, <reason>]`
    /// frame. Correlates against the per-relay pending kind:22242. On match,
    /// transitions to `Authenticated` (and flushes `AuthGate`'s buffered REQs
    /// back to outbound) or `Failed`. Non-AUTH OKs are no-ops here.
    ///
    /// T148: `delivering_relay_url` is the URL of the socket the OK arrived on;
    /// it is threaded into the lifecycle's per-URL `AuthGate` so the right
    /// per-URL pending buffer is drained on `Authenticated`. Pre-T148 this
    /// stamped `role.url()` (the lane bootstrap), which mis-routed the flush.
    pub(super) fn handle_auth_ok(
        &mut self,
        role: RelayRole,
        delivering_relay_url: &str,
        array: &[Value],
    ) -> Vec<OutboundMessage> {
        use super::super::auth::parse_ok_frame;

        let Some(ok) = parse_ok_frame(array) else {
            return Vec::new();
        };
        let driver = self.auth_drivers.entry(role).or_default();
        let Some(new_state) = driver.on_ok_frame(&ok) else {
            return Vec::new();
        };
        let relay_url = delivering_relay_url.to_string();
        let _gate_flushed = self
            .lifecycle
            .handle_auth_state_change(relay_url.clone(), new_state.clone());
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
        self.sync_transport_from_lane(role, delivering_relay_url);
        if matches!(new_state, RelayAuthState::Failed) {
            // T76 fail-closed: relay rejected our AUTH event — discard any
            // deferred REQs for this relay rather than leak them.
            self.purge_deferred_reqs_for(role);
        }
        self.log(format!("AUTH ok from {}: {new_state:?}", role.key()));
        if matches!(new_state, RelayAuthState::Authenticated) {
            // Re-issue all active plan subs for this relay via handle_reconnect.
            // This covers two cases:
            //   1. REQs buffered in the AuthGate (sent after challenge arrived):
            //      superseded by reconnect with current watermarks.
            //   2. REQs sent *before* the challenge arrived (state=NotRequired),
            //      which the relay CLOSED with "auth-required:" — those were
            //      never buffered, so the gate flush above is empty for them.
            //      handle_reconnect re-issues the full current plan to the relay.
            let replay = self.lifecycle.handle_reconnect(relay_url);
            let mut out = wire_frames_to_outbound(replay, role);
            // Finding B: a publish that hit `auth-required` on this relay was
            // PARKED (demoted to durable Pending in `unavailable_relays`) instead
            // of burning a retry budget. The relay is now authenticated, so it
            // can take the EVENT — re-dispatch the parked publish through the
            // SAME availability gate that reconnect uses for the read side. This
            // mirrors `kernel_reducer::handle_relay_connected`'s
            // `mark_publish_relay_available` call; both the read (REQ replay) and
            // write (publish re-dispatch) sides recover off one event-driven
            // transition (D8: no sleep/poll, no parallel auth-park mechanism).
            out.extend(self.mark_publish_relay_available(delivering_relay_url));
            out
        } else {
            wire_frames_to_outbound(_gate_flushed, role)
        }
    }

    /// Reflect the per-relay auth state into the diagnostic
    /// `RelayStatus.auth` field. AUTH-state transitions DO bump
    /// `changed_since_emit` so the diagnostic surface (`RelayStatus` + toast)
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
        use super::super::closed_reason::ERR_AUTH_REQUIRED;
        let key = auth_state_key(&state);
        let is_failed = matches!(state, RelayAuthState::Failed);
        let relay = self.relay_mut(role);
        relay.auth = key.to_string();
        // Typed FFI error contract — keep `error_category` in lockstep with
        // `last_error`:
        // - A Failed transition stamps both `last_error` (reason text) and
        //   `error_category = auth_required` so the host can prompt for
        //   credentials.
        // - A *recovery* transition (anything non-Failed) clears ONLY a
        //   stale `auth_required` category — it must not clobber a category
        //   set by a different surface (e.g. a `transient` from a CLOSED
        //   rate-limited frame that interleaved before this AUTH frame).
        //   `last_error` itself is owned by `relay_connected` / `relay_failed`
        //   for the non-Failed case, so leaving it untouched here is correct.
        if is_failed {
            if let Some(r) = reason {
                relay.last_error = Some(r);
            }
            relay.error_category = Some(ERR_AUTH_REQUIRED.to_string());
        } else if relay.error_category.as_deref() == Some(ERR_AUTH_REQUIRED) {
            relay.error_category = None;
        }
        // D8: bump the dirty flag so the diagnostic surface re-emits on the
        // next actor tick. The actor's emit-interval throttle (≤60 Hz/view)
        // bounds throughput; per-tick coalescing handles burst scenarios.
        self.changed_since_emit = true;
    }
}
