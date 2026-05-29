//! NIP-01 `CLOSED` frame ingest — parse, classify, route side-effect.
//!
//! Per the [relay-lifecycle review](../../../../docs/research/relay-lifecycle-and-pools.md)
//! G8/G11, NIP-01 `CLOSED` frames carry a machine-readable reason prefix
//! (`auth-required:`, `restricted:`, `rate-limited:`, …) that the kernel
//! must route to distinct actions: AUTH-pause vs back-off vs mark-denied
//! vs give-up. Before T120 the kernel folded every CLOSED to a generic
//! "`closed_by_relay`" — UI saw the reason string but the actor had no
//! signal to suppress retries against a denied relay or pause REQs to a
//! relay that just demanded AUTH.
//!
//! The classifier itself lives in [`crate::kernel::closed_reason`] (pure,
//! no kernel deps). This file is the **dispatch glue**: given a classified
//! reason it mutates kernel state and updates diagnostic surfaces.
//!
//! ## Action table
//!
//! | Reason            | Side effect                                             |
//! |-------------------|---------------------------------------------------------|
//! | `auth-required:`  | Pause this relay's REQs via `lifecycle.AuthGate`. Set   |
//! |                   | `relay.auth = "challenge_received"` so the diagnostic   |
//! |                   | surface reflects the demand. The actual AUTH wire frame |
//! |                   | (kind:22242) is built when the relay sends its own AUTH |
//! |                   | challenge — we do NOT synthesize a pseudo-challenge     |
//! |                   | from CLOSED (would violate NIP-42 replay protection).   |
//! | `rate-limited:`   | Stamp `last_error`, record `last_close_reason`.         |
//! |                   | Enqueue a `BackoffHint::RateLimited` in                 |
//! |                   | `pending_backoff_hints` so the actor can forward it to  |
//! |                   | the pool worker (V-58 fix).                             |
//! | `restricted:`     | Set `relay.denied = true`. Reconnect/REQ machinery      |
//! | `blocked:`        | treats `denied` as offline-for-this-client; recovery is |
//! | `shadowbanned:`   | a fresh socket only (relay edit / re-pay).              |
//! | `error:`          | Log + give up (no state change beyond `last_error` and  |
//! | `invalid:`        | `last_close_reason`). The sub is already marked         |
//! | `unsupported:`    | `closed_by_relay` by the calling `"CLOSED"` arm; we     |
//! | `pow:`            | just need not retry it.                                 |
//! | `duplicate:`      | Log + no state change beyond `last_close_reason`.       |
//! | `Unknown` prefix  | Treated as `error:` — log + give up.                    |
//!
//! D7 compliance: the wire delivers the frame; the kernel applies a
//! policy table. Capability layer has no knowledge of these reason codes.
//! D8 compliance: AUTH-state changes via this path do bump
//! `changed_since_emit` (the diagnostic surface must re-emit) — same
//! convention as `update_relay_auth_status` in `auth_handlers.rs`.

use super::super::closed_reason::{classify, CloseReason};
use super::super::{truncate, Kernel, RelayRole};
use crate::subs::RelayAuthState;

impl Kernel {
    /// Apply NIP-01 CLOSED reason-prefix policy: classify, mutate kernel
    /// state, and stamp diagnostic fields. Returns `true` when the
    /// classification triggered a state change that warrants
    /// `changed_since_emit` (the caller bumps the flag).
    ///
    /// `reason_text` is the truncated reason string already stored on the
    /// wire-sub by the calling `"CLOSED"` arm. `None` / empty / whitespace
    /// folds to [`CloseReason::Unknown`] which is treated as `error:`.
    pub(super) fn classify_and_route_closed(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        reason_text: Option<&str>,
    ) {
        let raw = reason_text.unwrap_or("");
        let class = classify(raw);

        match class {
            CloseReason::AuthRequired => {
                self.on_closed_auth_required(role, relay_url, sub_id, raw);
            }
            CloseReason::Restricted | CloseReason::Blocked | CloseReason::Shadowbanned => {
                self.on_closed_denied(role, sub_id, class, raw);
            }
            CloseReason::RateLimited => {
                self.on_closed_rate_limited(role, relay_url, sub_id, raw);
            }
            CloseReason::Error
            | CloseReason::Invalid
            | CloseReason::Unsupported
            | CloseReason::Pow
            | CloseReason::Unknown => {
                self.on_closed_give_up(role, sub_id, class, raw);
            }
            CloseReason::Duplicate => self.on_closed_duplicate(role, sub_id, raw),
        }
    }

    /// `auth-required:` — pause this relay via the lifecycle `AuthGate` and
    /// reflect the demand into `RelayStatus.auth`. The relay is expected to
    /// follow up with a real `["AUTH", challenge]` frame; the existing
    /// `handle_auth_challenge` path then drives signing. Synthesizing a
    /// pseudo-challenge here would break NIP-42 replay protection.
    ///
    /// T148: `relay_url` is the delivering socket's URL. Pre-T148 this
    /// stamped `role.url()` (the lane bootstrap), mis-keying the lifecycle's
    /// per-URL `AuthGate` and leaving the actual paused URL unguarded.
    fn on_closed_auth_required(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        raw: &str,
    ) {
        let _paused = self
            .lifecycle
            .handle_auth_state_change(relay_url.to_string(), RelayAuthState::ChallengeReceived);
        self.update_relay_auth_status(
            role,
            RelayAuthState::ChallengeReceived,
            Some(format!("auth-required (CLOSED {sub_id})")),
        );
        let relay = self.relay_mut(role);
        relay.last_close_reason = Some(CloseReason::AuthRequired.as_key().to_string());
        // `changed_since_emit` is already set by `update_relay_auth_status`.
        self.log(format!(
            "CLOSED auth-required from {} sub={sub_id}: {}",
            role.key(),
            truncate(raw, 120)
        ));
    }

    /// `restricted:` / `blocked:` / `shadowbanned:` — mark the relay denied
    /// for this client; the reconnect/REQ machinery suppresses retries.
    fn on_closed_denied(&mut self, role: RelayRole, sub_id: &str, class: CloseReason, raw: &str) {
        let key = class.as_key();
        let category = class.error_category();
        let relay = self.relay_mut(role);
        relay.denied = true;
        relay.last_close_reason = Some(key.to_string());
        relay.last_error = Some(format!("denied ({key}): {}", truncate(raw, 140)));
        relay.error_category = category.map(str::to_string);
        self.changed_since_emit = true;
        self.log(format!(
            "CLOSED {key} from {} sub={sub_id} — marking relay denied: {}",
            role.key(),
            truncate(raw, 120)
        ));
    }

    /// `rate-limited:` — record the classification for diagnostic purposes and
    /// enqueue a one-shot [`super::super::BackoffHint::RateLimited`] so the
    /// actor forwards it to the pool worker (V-58). The transport backoff
    /// machinery lives in the worker (`nmp-network`/G4); the kernel's only job
    /// here is to signal that a long backoff is warranted on the next reconnect.
    /// The hint is URL-keyed so the actor can route it to the right pool handle.
    fn on_closed_rate_limited(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        raw: &str,
    ) {
        let relay = self.relay_mut(role);
        relay.last_close_reason = Some(CloseReason::RateLimited.as_key().to_string());
        relay.last_error = Some(format!("rate-limited: {}", truncate(raw, 140)));
        relay.error_category = CloseReason::RateLimited
            .error_category()
            .map(str::to_string);
        self.changed_since_emit = true;
        // V-58: enqueue the backoff hint so the actor can forward it to the
        // pool worker on this URL. The hint is one-shot — the worker clears
        // it after applying it on the next disconnect.
        self.pending_backoff_hints.push((
            relay_url.to_string(),
            super::super::BackoffHint::RateLimited,
        ));
        self.log(format!(
            "CLOSED rate-limited from {} sub={sub_id}: {} (backoff hint enqueued)",
            role.key(),
            truncate(raw, 120)
        ));
    }

    /// `error:` / `invalid:` / `unsupported:` / `pow:` / unknown — log and
    /// give up. The sub is already marked `closed_by_relay` by the calling
    /// arm; we just record the classification so the UI can show why.
    fn on_closed_give_up(&mut self, role: RelayRole, sub_id: &str, class: CloseReason, raw: &str) {
        let key = class.as_key();
        let category = class.error_category();
        let relay = self.relay_mut(role);
        relay.last_close_reason = Some(key.to_string());
        relay.last_error = Some(format!("{key}: {}", truncate(raw, 140)));
        relay.error_category = category.map(str::to_string);
        self.changed_since_emit = true;
        self.log(format!(
            "CLOSED {key} from {} sub={sub_id}: {}",
            role.key(),
            truncate(raw, 120)
        ));
    }

    /// `duplicate:` — the relay says this REQ duplicates an existing sub.
    /// Diagnostic only; the calling arm has already marked the sub as
    /// `closed_by_relay`. No `last_error` (it's not really an error).
    fn on_closed_duplicate(&mut self, role: RelayRole, sub_id: &str, raw: &str) {
        let relay = self.relay_mut(role);
        relay.last_close_reason = Some(CloseReason::Duplicate.as_key().to_string());
        self.changed_since_emit = true;
        self.log(format!(
            "CLOSED duplicate from {} sub={sub_id}: {}",
            role.key(),
            truncate(raw, 120)
        ));
    }
}
