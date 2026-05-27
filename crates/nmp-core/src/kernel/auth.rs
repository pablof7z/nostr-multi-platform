//! NIP-42 AUTH wiring — kernel-internal handshake FSM and frame parsers.
//!
//! NIP-42 is wire-layer (the same category as NIP-01's EVENT/EOSE/OK/NOTICE/CLOSED
//! frames already parsed inline in `ingest/mod.rs::handle_text`). It is NOT an
//! app-domain protocol like NIP-29 / NIP-94 / NIP-77 which live in carved-out
//! crates per D0. The `nmp-nip42` crate (master @ e69c3a4) holds the canonical
//! standalone protocol module for downstream consumers and isolated validation;
//! the kernel inlines the FSM here because the AUTH handshake is tightly
//! coupled to the relay socket's wire path — the same circular-dependency
//! reasoning that keeps NIP-01 framing inline.
//!
//! Doctrine:
//! - **D0** — no app nouns. AUTH state is a transport-level capability.
//! - **D6** — errors are internal `Result`s, surfaced via the
//!   `RelayHealth.last_error` field; never crossed via FFI.
//! - **D7** — capabilities (the signer) report; this module decides FSM
//!   transitions and policy (what to emit on the wire, reconnect/retry).
//! - **D8** — AUTH-state transitions bump `changed_since_emit` so the
//!   diagnostic surface (`RelayStatus.auth`, `last_error`) re-emits, but
//!   they do NOT directly bump `kernel.rev` — that's done by `make_update`
//!   which the actor rate-caps at ≤60 Hz/view. AUTH-pause REQ re-defers
//!   use the silent `defer_outbound_silent` variant so the actor doesn't
//!   busy-wake every tick during a long AUTH-pause window.

use crate::subs::RelayAuthState;
use crate::substrate::{SignedEvent, UnsignedEvent};
use serde_json::Value;
use std::sync::Arc;

/// T77: the AUTH/OK frame parsers + shapes now live in the dependency-free
/// `nmp-nip42-types` substrate crate, shared verbatim with `nmp-nip42` (no
/// Cargo cycle — the types crate depends on nothing in the workspace).
/// `OkFrame` is retained as an alias for `AuthOk` so the kernel FSM's
/// `on_ok_frame(&OkFrame)` signature and the `OkFrame { .. }` test
/// constructors are unchanged.
pub(crate) use nmp_nip42_types::{parse_ok_frame, AuthOk as OkFrame};

/// Synchronous signer callback. The actor / iOS layer adapts
/// `nmp_signers::AccountManager::signer_active()` to this signature at kernel
/// construction time (avoids the `nmp-signers -> nmp-core` cycle that would
/// otherwise prevent direct trait use).
pub type AuthSignerFn = Arc<dyn Fn(&UnsignedEvent) -> Result<SignedEvent, String> + Send + Sync>;

/// Parsed `["AUTH", <challenge>]` frame → challenge string. Thin shim over
/// [`nmp_nip42_types::parse_auth_frame`]; the kernel only needs the
/// challenge value (the relay URL is the `RelayRole`, supplied by the
/// caller), so the relay-url arg is irrelevant here and passed empty.
pub(crate) fn parse_auth_challenge(frame: &[Value]) -> Option<String> {
    nmp_nip42_types::parse_auth_frame(frame, "").map(|c| c.challenge)
}

/// Build the kind:22242 unsigned event for AUTH per NIP-42. Two mandatory
/// tags: `["relay", <url>]` and `["challenge", <value>]`. The signer fills
/// `id`/`pubkey`/`sig`; the `pubkey` field on the unsigned template is the
/// signer's active pubkey hex (caller supplies).
pub(crate) fn build_auth_event(
    pubkey: String,
    relay_url: &str,
    challenge: &str,
    created_at: u64,
) -> UnsignedEvent {
    UnsignedEvent {
        pubkey,
        kind: 22242,
        tags: vec![
            vec!["relay".to_string(), relay_url.to_string()],
            vec!["challenge".to_string(), challenge.to_string()],
        ],
        content: String::new(),
        created_at,
    }
}

/// Structural validation of a signer's response to `build_auth_event`. Catches
/// buggy/malicious signers that mutate the kind, drop the challenge tag, or
/// return malformed ids/sigs. Mirrors `nmp_nip42::builder::validate_signed_for`
/// (inlined here for the same crate-cycle reason as the FSM itself). Returns
/// `Err` with a short reason on any structural divergence — the caller surfaces
/// it as `RelayAuthState::Failed` plus a toast-bound `failure_reason`.
///
/// Schnorr signature verification is the store's `VerifiedEvent::try_from_raw`
/// path, not this guard — this is the structural-shape gate only.
pub(crate) fn validate_signed_for(signed: &SignedEvent, challenge: &str) -> Result<(), String> {
    if signed.unsigned.kind != 22242 {
        return Err(format!(
            "signer returned wrong kind: expected 22242, got {}",
            signed.unsigned.kind
        ));
    }
    if signed.id.is_empty() || signed.id.len() != 64 {
        return Err(format!(
            "signer returned malformed id (len={})",
            signed.id.len()
        ));
    }
    if signed.sig.is_empty() || signed.sig.len() != 128 {
        return Err(format!(
            "signer returned malformed sig (len={})",
            signed.sig.len()
        ));
    }
    let has_challenge = signed
        .unsigned
        .tags
        .iter()
        .any(|tag| tag.len() >= 2 && tag[0] == "challenge" && tag[1] == challenge);
    if !has_challenge {
        return Err("signer dropped or mutated the challenge tag".to_string());
    }
    let has_relay = signed
        .unsigned
        .tags
        .iter()
        .any(|tag| tag.len() >= 2 && tag[0] == "relay" && !tag[1].is_empty());
    if !has_relay {
        return Err("signer dropped or mutated the relay tag".to_string());
    }
    Ok(())
}

/// Per-relay handshake driver state. One per `RelayRole`. Default is
/// `NotRequired`; an inbound AUTH transitions to `ChallengeReceived` and the
/// caller invokes the signer.
#[derive(Clone, Debug)]
pub(crate) struct AuthDriverState {
    pub state: RelayAuthState,
    pub pending_challenge: Option<String>,
    /// Event id of the in-flight kind:22242. Cleared on OK match or reset.
    pub pending_event_id: Option<String>,
}

impl Default for AuthDriverState {
    fn default() -> Self {
        Self {
            state: RelayAuthState::NotRequired,
            pending_challenge: None,
            pending_event_id: None,
        }
    }
}

impl AuthDriverState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset on relay disconnect — the next connect re-learns AUTH requirement
    /// from a fresh challenge.
    pub fn reset_on_disconnect(&mut self) {
        *self = Self::new();
    }

    /// Apply an inbound AUTH challenge. Always transitions to
    /// `ChallengeReceived` (re-auth mid-session drops back from `Authenticated`).
    pub fn on_auth_frame(&mut self, challenge: String) {
        self.pending_challenge = Some(challenge);
        self.pending_event_id = None;
        self.state = RelayAuthState::ChallengeReceived;
    }

    /// Record that a signed kind:22242 has been dispatched. Transitions to
    /// `Authenticating`. Returns `false` if no challenge is pending (caller
    /// race vs disconnect).
    pub fn record_dispatch(&mut self, signed_event_id: String) -> bool {
        if self.pending_challenge.is_none() {
            return false;
        }
        self.pending_event_id = Some(signed_event_id);
        self.state = RelayAuthState::Authenticating;
        true
    }

    /// Apply a signer failure (signer rejected, threw, or returned invalid).
    pub fn record_signer_failure(&mut self) {
        self.state = RelayAuthState::Failed;
        self.pending_event_id = None;
    }

    /// Apply an OK frame correlated against the in-flight event id. Returns
    /// the new state when matched, or `None` when the OK is for some other
    /// event (publish OK, unrelated id).
    pub fn on_ok_frame(&mut self, ok: &OkFrame) -> Option<RelayAuthState> {
        if self.pending_event_id.as_deref() != Some(ok.event_id.as_str()) {
            return None;
        }
        self.pending_event_id = None;
        let new_state = if ok.accepted {
            RelayAuthState::Authenticated
        } else {
            RelayAuthState::Failed
        };
        self.state = new_state.clone();
        Some(new_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_auth_extracts_non_empty_challenge() {
        assert_eq!(
            parse_auth_challenge(&[json!("AUTH"), json!("abc")]).as_deref(),
            Some("abc")
        );
        assert!(parse_auth_challenge(&[json!("AUTH"), json!("")]).is_none());
        assert!(parse_auth_challenge(&[json!("AUTH"), json!(42)]).is_none());
        assert!(parse_auth_challenge(&[json!("EVENT"), json!("x")]).is_none());
    }

    #[test]
    fn parse_ok_requires_strict_bool() {
        let id = "a".repeat(64);
        let f = parse_ok_frame(&[json!("OK"), json!(id), json!(true), json!("")]).unwrap();
        assert!(f.accepted);
        assert!(parse_ok_frame(&[json!("OK"), json!("a"), json!("true"), json!("")]).is_none());
        assert!(parse_ok_frame(&[json!("OK"), json!("a"), json!(1), json!("")]).is_none());
    }

    #[test]
    fn driver_records_dispatch_only_with_pending_challenge() {
        let mut d = AuthDriverState::new();
        assert!(!d.record_dispatch("evt1".into()));
        d.on_auth_frame("c1".into());
        assert!(d.record_dispatch("evt1".into()));
        assert_eq!(d.state, RelayAuthState::Authenticating);
    }

    #[test]
    fn driver_correlates_ok_against_pending_event_id() {
        let mut d = AuthDriverState::new();
        d.on_auth_frame("c1".into());
        d.record_dispatch("evt1".into());
        let unrelated = OkFrame {
            event_id: "evt2".into(),
            accepted: true,
            reason: String::new(),
        };
        assert!(d.on_ok_frame(&unrelated).is_none(), "unrelated OK = no-op");
        let auth_ok = OkFrame {
            event_id: "evt1".into(),
            accepted: true,
            reason: String::new(),
        };
        assert_eq!(d.on_ok_frame(&auth_ok), Some(RelayAuthState::Authenticated));
    }

    #[test]
    fn driver_failed_on_rejection() {
        let mut d = AuthDriverState::new();
        d.on_auth_frame("c1".into());
        d.record_dispatch("evt1".into());
        let rejected = OkFrame {
            event_id: "evt1".into(),
            accepted: false,
            reason: "restricted".into(),
        };
        assert_eq!(d.on_ok_frame(&rejected), Some(RelayAuthState::Failed));
    }

    #[test]
    fn build_auth_event_has_two_mandatory_tags() {
        let e = build_auth_event(
            "ff".repeat(32),
            "wss://relay.example",
            "challenge-value",
            123,
        );
        assert_eq!(e.kind, 22242);
        assert_eq!(e.tags.len(), 2);
        assert_eq!(e.tags[0], vec!["relay", "wss://relay.example"]);
        assert_eq!(e.tags[1], vec!["challenge", "challenge-value"]);
    }

    fn ok_signed(challenge: &str) -> SignedEvent {
        let unsigned = build_auth_event("ff".repeat(32), "wss://relay.example", challenge, 123);
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned,
        }
    }

    #[test]
    fn validate_signed_for_accepts_well_formed() {
        let s = ok_signed("ch1");
        assert!(validate_signed_for(&s, "ch1").is_ok());
    }

    #[test]
    fn validate_signed_for_rejects_wrong_kind() {
        let mut s = ok_signed("ch1");
        s.unsigned.kind = 1;
        let err = validate_signed_for(&s, "ch1").unwrap_err();
        assert!(err.contains("wrong kind"), "{err}");
    }

    #[test]
    fn validate_signed_for_rejects_dropped_challenge() {
        let mut s = ok_signed("ch1");
        s.unsigned
            .tags
            .retain(|t| t.first().map(|x| x.as_str()) != Some("challenge"));
        let err = validate_signed_for(&s, "ch1").unwrap_err();
        assert!(err.contains("challenge"), "{err}");
    }

    #[test]
    fn validate_signed_for_rejects_mismatched_challenge() {
        let s = ok_signed("ch1");
        let err = validate_signed_for(&s, "different-challenge").unwrap_err();
        assert!(err.contains("challenge"), "{err}");
    }

    #[test]
    fn validate_signed_for_rejects_malformed_id_or_sig() {
        let mut s = ok_signed("ch1");
        s.id = "tooshort".to_string();
        assert!(validate_signed_for(&s, "ch1").is_err());
        s.id = "a".repeat(64);
        s.sig = "tooshort".to_string();
        assert!(validate_signed_for(&s, "ch1").is_err());
    }
}
