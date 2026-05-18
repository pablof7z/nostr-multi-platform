//! `RelayAuthState` — the canonical NIP-42 per-relay lifecycle.
//!
//! Mirrors ADR-0007 §1 exactly. This is the authoritative type;
//! `nmp_core::subs::trigger::RelayAuthState` is a placeholder the M8-subs
//! task shipped so the trigger inbox and auth-gate could land without
//! waiting for this crate (see `docs/plan/m8-subscription-lifecycle.md` §3).
//!
//! Bidirectional conversion lives in this module so consumers can fan
//! transitions into the lifecycle inbox via `CompileTrigger::
//! RelayAuthStateChanged { state: subs::RelayAuthState, .. }` without
//! taking a direct dependency on this crate from `nmp-core::subs`.

use serde::{Deserialize, Serialize};

use nmp_core::subs::RelayAuthState as SubsRelayAuthState;

/// Per-relay NIP-42 lifecycle state.
///
/// Transitions:
///
/// ```text
///                     ┌───────────────┐
///                     │  NotRequired  │  (no AUTH challenge seen)
///                     └───────┬───────┘
///                             │ AUTH frame arrives
///                             ▼
///                ┌────────────────────────┐
///                │   ChallengeReceived    │  (signer not yet invoked)
///                └────────────┬───────────┘
///                             │ signer.sign() dispatched
///                             ▼
///                  ┌────────────────────┐
///                  │   Authenticating   │  (kind:22242 on the wire)
///                  └─────┬──────────┬───┘
///                    OK true     OK false / signer error
///                        │            │
///                        ▼            ▼
///              ┌──────────────┐  ┌──────────┐
///              │ Authenticated │  │  Failed  │  (next reconnect resets)
///              └──────────────┘  └──────────┘
/// ```
///
/// A relay disconnect resets the state to `NotRequired`; the relay will
/// re-send a fresh challenge if it still requires AUTH on the next connect.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayAuthState {
    /// No AUTH challenge has arrived for this relay. Subscriptions flow
    /// unimpeded.
    #[default]
    NotRequired,
    /// Relay sent AUTH; the signer has not yet produced a kind:22242. The
    /// driver may be waiting on the signer being bound, or the signer is
    /// returning a `SignerOp::Pending` result.
    ChallengeReceived,
    /// We dispatched a kind:22242 and are awaiting `["OK", <event_id>,
    /// true|false, <reason>]` from the relay.
    Authenticating,
    /// Relay accepted our AUTH event. Subscriptions resume; `subs::AuthGate`
    /// drains its held REQs on this transition.
    Authenticated,
    /// Signer refused to sign, or the relay rejected our AUTH event. Stays
    /// held until a reconnect resets state to `NotRequired` and the new
    /// challenge triggers a fresh attempt.
    Failed,
}

impl RelayAuthState {
    /// Wire key the diagnostics UI displays in `RelayStatus.auth`. Matches
    /// the snake-case serialization expected by ADR-0007.
    pub fn as_status_key(&self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::ChallengeReceived => "challenge_received",
            Self::Authenticating => "authenticating",
            Self::Authenticated => "authenticated",
            Self::Failed => "failed",
        }
    }
}

/// Translate from the canonical M5 type to the `nmp-core::subs::trigger`
/// placeholder. This is what callers fan into the lifecycle's
/// `CompileTrigger::RelayAuthStateChanged` variant.
pub fn relay_auth_state_to_subs(state: &RelayAuthState) -> SubsRelayAuthState {
    match state {
        RelayAuthState::NotRequired => SubsRelayAuthState::NotRequired,
        RelayAuthState::ChallengeReceived => SubsRelayAuthState::ChallengeReceived,
        RelayAuthState::Authenticating => SubsRelayAuthState::Authenticating,
        RelayAuthState::Authenticated => SubsRelayAuthState::Authenticated,
        RelayAuthState::Failed => SubsRelayAuthState::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_keys_match_adr_0007() {
        assert_eq!(RelayAuthState::NotRequired.as_status_key(), "not_required");
        assert_eq!(
            RelayAuthState::ChallengeReceived.as_status_key(),
            "challenge_received"
        );
        assert_eq!(
            RelayAuthState::Authenticating.as_status_key(),
            "authenticating"
        );
        assert_eq!(
            RelayAuthState::Authenticated.as_status_key(),
            "authenticated"
        );
        assert_eq!(RelayAuthState::Failed.as_status_key(), "failed");
    }

    #[test]
    fn subs_translation_is_total_and_lossless() {
        for state in [
            RelayAuthState::NotRequired,
            RelayAuthState::ChallengeReceived,
            RelayAuthState::Authenticating,
            RelayAuthState::Authenticated,
            RelayAuthState::Failed,
        ] {
            let subs = relay_auth_state_to_subs(&state);
            // The two enums share variant names; the round-trip is
            // self-evident from the match. Pin the discriminator names so a
            // rename in either crate breaks here loudly.
            let subs_key = format!("{subs:?}").to_lowercase();
            let local_key = format!("{state:?}").to_lowercase();
            assert_eq!(
                subs_key, local_key,
                "subs / nip42 RelayAuthState diverged on variant {state:?}",
            );
        }
    }

    #[test]
    fn default_is_not_required() {
        assert_eq!(RelayAuthState::default(), RelayAuthState::NotRequired);
    }
}
