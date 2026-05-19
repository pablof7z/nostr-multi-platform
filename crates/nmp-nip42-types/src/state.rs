//! `RelayAuthState` — the canonical per-relay NIP-42 lifecycle enum.
//!
//! Before T77 this existed twice: `nmp_core::subs::trigger::RelayAuthState`
//! (derives `Hash`, no serde) and `nmp_nip42::state::RelayAuthState`
//! (derives serde + `Default`, no `Hash`), bridged by a hand-written
//! `relay_auth_state_to_subs` translation function whose only job was to
//! assert the two stayed variant-identical. This is the single type; the
//! derive set is the union of both prior call sites' needs.

use serde::{Deserialize, Serialize};

/// Per-relay NIP-42 lifecycle state.
///
/// Transitions:
///
/// ```text
///   NotRequired ──AUTH frame──▶ ChallengeReceived ──signer dispatched──▶
///   Authenticating ──OK true──▶ Authenticated
///                  └─OK false / signer error──▶ Failed
/// ```
///
/// A relay disconnect resets the state to `NotRequired`; the relay will
/// re-send a fresh challenge if it still requires AUTH on the next connect.
///
/// `Failed` is **fail-closed** (ADR-0019): a relay that demanded AUTH and
/// failed it withholds its gated REQs rather than silently downgrading to
/// unauthenticated reads. Recovery is reconnect-only.
#[derive(
    Clone, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RelayAuthState {
    /// No AUTH challenge has arrived for this relay. Subscriptions flow
    /// unimpeded.
    #[default]
    NotRequired,
    /// Relay sent AUTH; the signer has not yet produced a kind:22242.
    ChallengeReceived,
    /// We dispatched a kind:22242 and are awaiting
    /// `["OK", <event_id>, true|false, <reason>]` from the relay.
    Authenticating,
    /// Relay accepted our AUTH event. Subscriptions resume.
    Authenticated,
    /// Signer refused to sign, or the relay rejected our AUTH event.
    /// Fail-closed: gated REQs are withheld until a reconnect resets state
    /// to `NotRequired` and a fresh challenge triggers a new attempt.
    Failed,
}

impl RelayAuthState {
    /// Wire key the diagnostics UI displays in `RelayStatus.auth`. Matches
    /// the snake-case serde serialization (ADR-0007 §1).
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
    fn default_is_not_required() {
        assert_eq!(RelayAuthState::default(), RelayAuthState::NotRequired);
    }

    #[test]
    fn serde_uses_snake_case_matching_status_key() {
        for st in [
            RelayAuthState::NotRequired,
            RelayAuthState::ChallengeReceived,
            RelayAuthState::Authenticating,
            RelayAuthState::Authenticated,
            RelayAuthState::Failed,
        ] {
            let json = serde_json::to_string(&st).unwrap();
            assert_eq!(json, format!("\"{}\"", st.as_status_key()));
            let back: RelayAuthState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, st);
        }
    }
}
