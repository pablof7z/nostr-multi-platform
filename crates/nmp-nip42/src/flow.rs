//! NIP-42 handshake driver — per-relay state machine.
//!
//! One [`Nip42Driver`] instance per relay. The driver holds the in-flight
//! challenge + pending kind:22242 event id, and turns frames + signer
//! results into [`HandshakeOutcome`] values the caller acts on.
//!
//! The caller (the kernel's `handle_text`, in production) is responsible for:
//! - Calling [`Nip42Driver::on_auth_frame`] when an `["AUTH", _]` arrives.
//! - Calling [`Nip42Driver::on_ok_frame`] for every `["OK", _, _, _]`
//!   (the driver checks event-id match and is a no-op for non-AUTH OKs).
//! - Fanning [`HandshakeOutcome::wire_frames`] back through the relay socket
//!   verbatim — already-formatted `["AUTH", <event>]` JSON strings.
//! - Fanning the new [`RelayAuthState`] into the subs lifecycle inbox via
//!   `CompileTrigger::RelayAuthStateChanged` (the actual emit site is the
//!   M2-phase-2 actor wiring task; for now the kernel can call
//!   [`relay_auth_state_to_subs`](super::state::relay_auth_state_to_subs)
//!   to translate).
//!
//! Signer integration is via a small trait the caller adapts to either
//! `nmp_core::publish::traits::Signer::sign_auth` (the M7 shim) or
//! `nmp_signers::Signer::sign` (the M6 canonical trait). Both will be
//! available; the choice is the caller's because the two have different
//! lifetimes and SignerOp return shapes.

use nmp_core::substrate::SignedEvent;

use super::builder::{build_auth_event, validate_signed_for, wire_frame_for};
use super::frame::{AuthChallenge, AuthOk};
use super::state::RelayAuthState;

/// What the driver returns from each tick. Wire frames must be sent to
/// the relay in order; the state transition (if any) drives diagnostics
/// and the `subs::AuthGate` REQ pause/flush via the lifecycle inbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandshakeOutcome {
    /// Already-rendered JSON wire frames the caller pushes to the relay.
    /// Currently always 0 or 1 frame (the kind:22242 dispatch); kept as a
    /// Vec so future protocol additions (e.g. AUTH-CANCEL) don't break the
    /// surface.
    pub wire_frames: Vec<String>,
    /// New state for the relay's lifecycle, if the tick caused a
    /// transition. `None` means no diagnostic state change (idempotent
    /// frame, no-op signer result, etc.).
    pub new_state: Option<RelayAuthState>,
    /// When the new_state is `Failed`, the human-readable reason. Caller
    /// surfaces it as a toast (M10.5 toast-field bridge) and/or logs it.
    pub failure_reason: Option<String>,
}

impl HandshakeOutcome {
    fn empty() -> Self {
        Self::default()
    }

    fn transition_to(state: RelayAuthState) -> Self {
        Self {
            new_state: Some(state),
            ..Self::default()
        }
    }

    fn failure(reason: impl Into<String>) -> Self {
        Self {
            new_state: Some(RelayAuthState::Failed),
            failure_reason: Some(reason.into()),
            ..Self::default()
        }
    }
}

/// Errors the driver returns for its internal `Result`s. Never crosses
/// FFI per D6 — converts to `RelayAuthState::Failed` plus a reason in
/// [`HandshakeOutcome`].
#[derive(Clone, Debug)]
pub enum Nip42Error {
    /// The signer was invoked but reported failure or unavailability.
    SignerFailed(String),
    /// The signer returned a structurally invalid event (wrong kind,
    /// missing challenge echo, malformed id, etc.). Catches buggy or
    /// malicious signers.
    SignerReturnedInvalid(String),
}

impl std::fmt::Display for Nip42Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignerFailed(m) => write!(f, "signer failed: {m}"),
            Self::SignerReturnedInvalid(m) => write!(f, "signer returned invalid event: {m}"),
        }
    }
}

/// Per-relay handshake state. Default-constructed; the caller owns one of
/// these per relay URL and feeds it frames.
#[derive(Clone, Debug, Default)]
pub struct Nip42Driver {
    state: RelayAuthState,
    challenge: Option<AuthChallenge>,
    /// The event id of the in-flight kind:22242. None until we dispatch;
    /// cleared on OK match or reset.
    pending_event_id: Option<String>,
}

impl Nip42Driver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &RelayAuthState {
        &self.state
    }

    pub fn pending_challenge(&self) -> Option<&AuthChallenge> {
        self.challenge.as_ref()
    }

    /// Reset the driver. Called on relay disconnect — the next connect
    /// re-learns whether AUTH is required from a fresh challenge.
    pub fn reset_on_disconnect(&mut self) {
        self.state = RelayAuthState::NotRequired;
        self.challenge = None;
        self.pending_event_id = None;
    }

    /// Handle a parsed `["AUTH", challenge]` frame.
    ///
    /// Transitions state to `ChallengeReceived`. The caller invokes the
    /// signer (synchronously, via `LocalKeySigner`, or asynchronously via
    /// `Nip46Signer` polling) and then calls
    /// [`Nip42Driver::deliver_signed`] with the result. The driver
    /// produces no wire frames on this call — those land when the signed
    /// event arrives.
    pub fn on_auth_frame(&mut self, challenge: AuthChallenge) -> HandshakeOutcome {
        self.challenge = Some(challenge);
        self.pending_event_id = None;
        // If we were already Authenticated and the relay sent a fresh
        // challenge (re-auth mid-session), drop back to ChallengeReceived.
        self.state = RelayAuthState::ChallengeReceived;
        HandshakeOutcome::transition_to(RelayAuthState::ChallengeReceived)
    }

    /// Deliver a signer result for the current challenge.
    ///
    /// `Ok(signed)` → validates the signed event structurally, dispatches
    /// the wire frame, transitions to `Authenticating`.
    ///
    /// `Err(reason)` → transitions to `Failed` with the reason.
    ///
    /// Idempotent: if there is no pending challenge (e.g. caller raced a
    /// disconnect), returns an empty outcome.
    ///
    /// # Race-safety
    ///
    /// This method does NOT correlate the signer result with the challenge
    /// the signer was originally invoked for. That is safe for the
    /// synchronous path used by [`run_handshake`] — the signer call
    /// happens between `on_auth_frame` and `deliver_signed` in the same
    /// stack frame. **For async signers** (NIP-46 bunker via
    /// `SignerOp::Pending` polling) use [`Self::deliver_signed_for`] with
    /// the challenge the signer was invoked for. If a fresh AUTH challenge
    /// arrives mid-handshake (relay reconnect, relay-initiated re-auth)
    /// the stale signer result will be silently discarded by the
    /// challenge-match check there.
    pub fn deliver_signed(&mut self, result: Result<SignedEvent, Nip42Error>) -> HandshakeOutcome {
        let Some(challenge) = self.challenge.clone() else {
            return HandshakeOutcome::empty();
        };
        if !matches!(self.state, RelayAuthState::ChallengeReceived) {
            return HandshakeOutcome::empty();
        }
        self.apply_signed_result(challenge, result)
    }

    /// Deliver a signer result correlated to a specific challenge value.
    ///
    /// Returns an empty outcome (no state change) when `expected_challenge`
    /// no longer matches the driver's current in-flight challenge — the
    /// canonical async-race guard. Use this from any signer path that can
    /// resolve out-of-order with `on_auth_frame` (NIP-46 bunker, threaded
    /// Keychain prompts, etc.).
    ///
    /// `expected_challenge` is the challenge string the signer was invoked
    /// for. Callers obtain it from [`Self::pending_challenge`] before
    /// dispatching the signer call and pass the same value here when the
    /// result returns.
    pub fn deliver_signed_for(
        &mut self,
        expected_challenge: &str,
        result: Result<SignedEvent, Nip42Error>,
    ) -> HandshakeOutcome {
        let Some(challenge) = self.challenge.clone() else {
            return HandshakeOutcome::empty();
        };
        if challenge.challenge != expected_challenge
            || !matches!(self.state, RelayAuthState::ChallengeReceived)
        {
            return HandshakeOutcome::empty();
        }
        self.apply_signed_result(challenge, result)
    }

    fn apply_signed_result(
        &mut self,
        challenge: AuthChallenge,
        result: Result<SignedEvent, Nip42Error>,
    ) -> HandshakeOutcome {
        match result {
            Ok(signed) => match validate_signed_for(&signed, &challenge) {
                Ok(()) => {
                    let wire = wire_frame_for(&signed);
                    self.pending_event_id = Some(signed.id);
                    self.state = RelayAuthState::Authenticating;
                    HandshakeOutcome {
                        wire_frames: vec![wire],
                        new_state: Some(RelayAuthState::Authenticating),
                        failure_reason: None,
                    }
                }
                Err(why) => {
                    self.state = RelayAuthState::Failed;
                    HandshakeOutcome::failure(format!("{}", Nip42Error::SignerReturnedInvalid(why)))
                }
            },
            Err(err) => {
                self.state = RelayAuthState::Failed;
                HandshakeOutcome::failure(err.to_string())
            }
        }
    }

    /// Handle a parsed `["OK", event_id, accepted, reason]` frame.
    ///
    /// No-op when:
    /// - We are not awaiting an AUTH ack (`state != Authenticating`).
    /// - The event_id doesn't match our pending kind:22242 (the OK is for
    ///   something else — a publish from the M7 engine, etc.).
    ///
    /// When matched, transitions to `Authenticated` on `accepted = true`
    /// or `Failed` on `accepted = false`.
    pub fn on_ok_frame(&mut self, ok: &AuthOk) -> HandshakeOutcome {
        let matches_pending = self
            .pending_event_id
            .as_deref()
            .is_some_and(|pending| pending == ok.event_id);
        if !matches_pending {
            return HandshakeOutcome::empty();
        }
        self.pending_event_id = None;
        if ok.accepted {
            self.state = RelayAuthState::Authenticated;
            HandshakeOutcome::transition_to(RelayAuthState::Authenticated)
        } else {
            self.state = RelayAuthState::Failed;
            HandshakeOutcome::failure(if ok.reason.is_empty() {
                "relay rejected AUTH".to_string()
            } else {
                format!("relay rejected AUTH: {}", ok.reason)
            })
        }
    }
}

/// Helper that bundles the canonical flow for callers that have a signer
/// callback in hand at challenge time: parse the AUTH frame, build the
/// unsigned event, invoke the signer, dispatch the result through
/// [`Nip42Driver::deliver_signed`]. Returns the merged outcome.
///
/// `signer` receives the unsigned event and returns either a signed event
/// or a `Nip42Error::SignerFailed`. The caller chooses how to bridge to
/// `nmp_core::publish::traits::Signer::sign_auth` (returns `SignedEvent`
/// synchronously) or `nmp_signers::Signer::sign` (returns `SignerOp`).
pub fn run_handshake<F>(
    driver: &mut Nip42Driver,
    challenge: AuthChallenge,
    pubkey: String,
    created_at: u64,
    mut signer: F,
) -> HandshakeOutcome
where
    F: FnMut(&nmp_core::substrate::UnsignedEvent) -> Result<SignedEvent, Nip42Error>,
{
    let mut outcome = driver.on_auth_frame(challenge.clone());
    let unsigned = build_auth_event(&challenge, pubkey, created_at);
    let result = signer(&unsigned);
    let delivered = driver.deliver_signed(result);
    // Prefer the second tick's state/reason since deliver_signed always
    // emits one (or empty when racing a disconnect).
    if delivered != HandshakeOutcome::empty() {
        outcome = delivered;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::UnsignedEvent;

    fn challenge_for(relay: &str, challenge: &str) -> AuthChallenge {
        AuthChallenge {
            challenge: challenge.to_string(),
            relay_url: relay.to_string(),
        }
    }

    fn good_signer_returning(
        id: &str,
    ) -> impl FnMut(&UnsignedEvent) -> Result<SignedEvent, Nip42Error> {
        let id = id.to_string();
        move |unsigned| {
            Ok(SignedEvent {
                id: id.clone(),
                sig: "c".repeat(128),
                unsigned: unsigned.clone(),
            })
        }
    }

    #[test]
    fn happy_path_drives_through_full_lifecycle() {
        let mut driver = Nip42Driver::new();
        assert_eq!(*driver.state(), RelayAuthState::NotRequired);

        // AUTH challenge arrives.
        let ch = challenge_for("wss://r", "abc");
        let outcome = driver.on_auth_frame(ch.clone());
        assert_eq!(outcome.new_state, Some(RelayAuthState::ChallengeReceived));
        assert!(outcome.wire_frames.is_empty());
        assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);

        // Signer returns a valid signed event.
        let id = "a".repeat(64);
        let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
        let signed = SignedEvent {
            id: id.clone(),
            sig: "c".repeat(128),
            unsigned,
        };
        let outcome = driver.deliver_signed(Ok(signed));
        assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
        assert_eq!(outcome.wire_frames.len(), 1);
        assert!(outcome.wire_frames[0].starts_with("[\"AUTH\","));
        assert_eq!(*driver.state(), RelayAuthState::Authenticating);

        // Relay accepts.
        let ok = AuthOk {
            event_id: id,
            accepted: true,
            reason: String::new(),
        };
        let outcome = driver.on_ok_frame(&ok);
        assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticated));
        assert!(outcome.wire_frames.is_empty());
        assert_eq!(*driver.state(), RelayAuthState::Authenticated);
        assert!(outcome.failure_reason.is_none());
    }

    #[test]
    fn rejected_ok_surfaces_reason_and_transitions_to_failed() {
        let mut driver = Nip42Driver::new();
        let ch = challenge_for("wss://r", "x");
        driver.on_auth_frame(ch.clone());
        let id = "b".repeat(64);
        let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
        let signed = SignedEvent {
            id: id.clone(),
            sig: "c".repeat(128),
            unsigned,
        };
        driver.deliver_signed(Ok(signed));

        let ok = AuthOk {
            event_id: id,
            accepted: false,
            reason: "restricted: subscribers only".to_string(),
        };
        let outcome = driver.on_ok_frame(&ok);
        assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
        assert_eq!(*driver.state(), RelayAuthState::Failed);
        let reason = outcome.failure_reason.unwrap();
        assert!(reason.contains("restricted"));
    }

    #[test]
    fn signer_failure_surfaces_without_dispatching_wire_frame() {
        let mut driver = Nip42Driver::new();
        let ch = challenge_for("wss://r", "x");
        driver.on_auth_frame(ch);
        let outcome =
            driver.deliver_signed(Err(Nip42Error::SignerFailed("keychain locked".to_string())));
        assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
        assert!(outcome.wire_frames.is_empty());
        let reason = outcome.failure_reason.unwrap();
        assert!(reason.contains("keychain locked"));
        assert_eq!(*driver.state(), RelayAuthState::Failed);
    }

    #[test]
    fn signer_returning_invalid_event_is_treated_as_failure() {
        let mut driver = Nip42Driver::new();
        let ch = challenge_for("wss://r", "x");
        driver.on_auth_frame(ch);
        let bad_signed = SignedEvent {
            id: "a".repeat(64),
            sig: "c".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "b".repeat(64),
                kind: 1, // wrong kind
                tags: vec![],
                content: String::new(),
                created_at: 1,
            },
        };
        let outcome = driver.deliver_signed(Ok(bad_signed));
        assert_eq!(outcome.new_state, Some(RelayAuthState::Failed));
        assert!(outcome.wire_frames.is_empty());
        assert!(outcome.failure_reason.unwrap().contains("expected 22242"));
    }

    #[test]
    fn unrelated_ok_does_not_change_state() {
        let mut driver = Nip42Driver::new();
        let ch = challenge_for("wss://r", "x");
        driver.on_auth_frame(ch.clone());
        let unsigned = build_auth_event(&ch, "p".repeat(64), 1);
        driver.deliver_signed(Ok(SignedEvent {
            id: "1".repeat(64),
            sig: "c".repeat(128),
            unsigned,
        }));
        assert_eq!(*driver.state(), RelayAuthState::Authenticating);

        // OK for a different event id — must be a no-op.
        let other = AuthOk {
            event_id: "9".repeat(64),
            accepted: true,
            reason: String::new(),
        };
        let outcome = driver.on_ok_frame(&other);
        assert_eq!(outcome, HandshakeOutcome::empty());
        assert_eq!(*driver.state(), RelayAuthState::Authenticating);
    }

    #[test]
    fn reset_on_disconnect_clears_state_and_challenge() {
        let mut driver = Nip42Driver::new();
        driver.on_auth_frame(challenge_for("wss://r", "x"));
        driver.reset_on_disconnect();
        assert_eq!(*driver.state(), RelayAuthState::NotRequired);
        assert!(driver.pending_challenge().is_none());
    }

    #[test]
    fn re_auth_after_authenticated_drops_back_to_challenge_received() {
        let mut driver = Nip42Driver::new();
        let ch1 = challenge_for("wss://r", "first");
        let id1 = "1".repeat(64);
        run_handshake(
            &mut driver,
            ch1.clone(),
            "p".repeat(64),
            1,
            good_signer_returning(&id1),
        );
        driver.on_ok_frame(&AuthOk {
            event_id: id1,
            accepted: true,
            reason: String::new(),
        });
        assert_eq!(*driver.state(), RelayAuthState::Authenticated);

        // Relay sends a fresh challenge mid-session.
        let ch2 = challenge_for("wss://r", "second");
        let outcome = driver.on_auth_frame(ch2);
        assert_eq!(outcome.new_state, Some(RelayAuthState::ChallengeReceived));
        assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
    }

    #[test]
    fn run_handshake_dispatches_through_signer_in_one_call() {
        let mut driver = Nip42Driver::new();
        let id = "7".repeat(64);
        let outcome = run_handshake(
            &mut driver,
            challenge_for("wss://r", "ch"),
            "p".repeat(64),
            1,
            good_signer_returning(&id),
        );
        assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
        assert_eq!(outcome.wire_frames.len(), 1);
        assert!(outcome.wire_frames[0].contains(&id));
    }

    #[test]
    fn deliver_signed_for_rejects_stale_signer_result() {
        let mut driver = Nip42Driver::new();
        let ch1 = challenge_for("wss://r", "first");
        driver.on_auth_frame(ch1.clone());
        // Hand the signer the first challenge. Before it returns, the
        // relay sends a fresh challenge (reconnect, re-auth mid-session).
        let ch2 = challenge_for("wss://r", "second");
        driver.on_auth_frame(ch2);
        assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
        // First signer call resolves late — must be discarded.
        let id = "1".repeat(64);
        let mut signer = good_signer_returning(&id);
        let stale = signer(&build_auth_event(&ch1, "p".repeat(64), 1));
        let outcome = driver.deliver_signed_for(&ch1.challenge, stale);
        assert_eq!(
            outcome,
            HandshakeOutcome::empty(),
            "stale signer result must be discarded"
        );
        assert_eq!(*driver.state(), RelayAuthState::ChallengeReceived);
        assert_eq!(
            driver.pending_challenge().unwrap().challenge,
            "second",
            "current challenge unchanged by stale delivery"
        );
    }

    #[test]
    fn deliver_signed_for_accepts_current_challenge() {
        let mut driver = Nip42Driver::new();
        let ch = challenge_for("wss://r", "live");
        driver.on_auth_frame(ch.clone());
        let id = "8".repeat(64);
        let mut signer = good_signer_returning(&id);
        let signed = signer(&build_auth_event(&ch, "p".repeat(64), 1));
        let outcome = driver.deliver_signed_for(&ch.challenge, signed);
        assert_eq!(outcome.new_state, Some(RelayAuthState::Authenticating));
        assert_eq!(outcome.wire_frames.len(), 1);
        assert_eq!(*driver.state(), RelayAuthState::Authenticating);
    }

    #[test]
    fn deliver_signed_without_pending_challenge_is_noop() {
        let mut driver = Nip42Driver::new();
        let outcome = driver.deliver_signed(Ok(SignedEvent {
            id: "a".repeat(64),
            sig: "c".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "b".repeat(64),
                kind: 22242,
                tags: vec![],
                content: String::new(),
                created_at: 1,
            },
        }));
        assert_eq!(outcome, HandshakeOutcome::empty());
        assert_eq!(*driver.state(), RelayAuthState::NotRequired);
    }
}
