//! Per-(event, relay) state machine and retry policy.
//!
//! State graph:
//! ```text
//!   Pending --send--> InFlight --ok--> Ok
//!      ^                  |
//!      |              +---+----+
//!      |              |        |
//!   (retry)       RelayError  Timeout
//!      |              |        |
//!      +------backoff-+--------+
//!                     |        |
//!                  (give up after N retries)
//!                     v
//!              FailedAfterRetries
//! ```
//!
//! The state machine is pure: it never holds wall-clock state, never spawns
//! threads, and never speaks to relays. It computes the next move from
//! `(state, ack, retry_policy, now_ms)`. The engine drives time.

use serde::{Deserialize, Serialize};

use super::action::RelayUrl;

/// Raw relay acknowledgement as reported by the dispatcher.
///
/// Per D7 (capabilities report, never decide), the dispatcher reports the
/// transport-level facts of the response and never tells the engine what to
/// do about it. The shape mirrors what a NIP-20 `OK` frame plus transport
/// metadata can carry:
///
/// - `ok`: the protocol-level boolean from the relay (`true` for OK, `false`
///   for NOTICE / OK-false / closed / timeout).
/// - `code`: a stable lowercased token derived from the NIP-20 prefix
///   (`"blocked"`, `"pow"`, `"rate-limited"`, `"auth-required"`, `"invalid"`,
///   `"error"`, `""`) or a transport-class token (`"timeout"`, `"io"`,
///   `"connection-reset"`). Empty for a clean `ok=true`.
/// - `message`: the human-readable string the relay (or transport) supplied.
/// - `details`: optional structured detail the relay surfaced (NIP-42
///   challenge, NIP-13 difficulty, retry-after, etc.). Most relays will leave
///   this `None`; the engine never requires it.
///
/// Classification into `AckClass` is the engine's job — see
/// [`classify_ack`]. Per D7 this struct deliberately carries no policy
/// hints (no retry/give-up enum variant, no `is_transient` flag).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RelayAck {
    pub relay_url: RelayUrl,
    pub ok: bool,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl RelayAck {
    /// Convenience constructor for the OK path.
    pub fn ok(relay_url: impl Into<RelayUrl>) -> Self {
        Self {
            relay_url: relay_url.into(),
            ok: true,
            code: String::new(),
            message: String::new(),
            details: None,
        }
    }

    /// Convenience constructor for the timeout path (transport-class failure).
    pub fn timed_out(relay_url: impl Into<RelayUrl>) -> Self {
        Self {
            relay_url: relay_url.into(),
            ok: false,
            code: TIMEOUT_CODE.to_string(),
            message: "no response from relay".to_string(),
            details: None,
        }
    }

    /// Convenience constructor for an OK-false / NOTICE failure with a
    /// caller-supplied code + message.
    pub fn failed(
        relay_url: impl Into<RelayUrl>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            relay_url: relay_url.into(),
            ok: false,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
}

/// Reserved code tokens recognised by [`classify_ack`]. Other tokens fall
/// through to `AckClass::Transient` (conservative retry).
const TIMEOUT_CODE: &str = "timeout";
const AUTH_REQUIRED_CODE: &str = "auth-required";

/// Permanent NIP-20 OK-false prefixes (engine gives up on these immediately).
const PERMANENT_CODES: &[&str] = &[
    "blocked",
    "pow",
    "rate-limited",
    "restricted",
    "invalid",
    "duplicate",
];

/// Engine-internal classification of a raw ack. Drives retry policy without
/// crossing the dispatcher boundary (per D7: policy is Rust's; capabilities
/// are reports). Visibility is crate-local because no caller outside the
/// publish engine should be making this judgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AckClass {
    /// `auth-required` — re-auth via the active signer, retry once.
    AuthRequired,
    /// Connection drop, socket reset, timeout, transient I/O — retry with
    /// backoff. Default verdict for unknown codes (conservative).
    Transient,
    /// `blocked` / `pow` / `rate-limited` / `restricted` / `invalid` /
    /// `duplicate` — permanent rejection; do not retry, surface to the
    /// snapshot. Also: a successful ack (`ok=true`) is conceptually permanent
    /// but never reaches the classifier (the engine routes it to `Settled(Ok)`
    /// without consulting `AckClass`).
    Permanent,
}

/// Classify a raw ack into the engine's retry-policy verdict. Pure function;
/// the engine is the only caller. Per D7 the dispatcher must never call this.
pub(crate) fn classify_ack(ack: &RelayAck) -> AckClass {
    if ack.ok {
        // Ok paths are handled by `apply_ack` directly without consulting
        // the classifier. Pin to Permanent so any accidental classifier call
        // on a success doesn't trigger a retry loop.
        return AckClass::Permanent;
    }
    let code = ack.code.as_str();
    if code == AUTH_REQUIRED_CODE {
        return AckClass::AuthRequired;
    }
    if PERMANENT_CODES.contains(&code) {
        return AckClass::Permanent;
    }
    // Includes "timeout", "io", "connection-reset", and any unknown token —
    // conservative default is to retry once with backoff.
    AckClass::Transient
}

/// Per-relay state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PerRelayState {
    Pending,
    InFlight {
        sent_at_ms: u64,
        attempt: u32,
    },
    Ok {
        acked_at_ms: u64,
    },
    RelayError {
        message: String,
        attempt: u32,
        last_at_ms: u64,
    },
    TimedOut {
        attempt: u32,
        last_at_ms: u64,
    },
    FailedAfterRetries {
        reason: String,
        last_at_ms: u64,
    },
}

impl PerRelayState {
    #[must_use] 
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Ok { .. } | Self::FailedAfterRetries { .. })
    }

    #[must_use] 
    pub fn attempt(&self) -> u32 {
        match self {
            Self::Pending => 0,
            Self::InFlight { attempt, .. }
            | Self::RelayError { attempt, .. }
            | Self::TimedOut { attempt, .. } => *attempt,
            Self::Ok { .. } | Self::FailedAfterRetries { .. } => 0,
        }
    }
}

/// One attempted send. Owned by the engine; persisted via `PublishStore`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PublishAttempt {
    pub relay_url: RelayUrl,
    pub state: PerRelayState,
}

/// What the planner produced for a single publish (one entry per resolved
/// relay). Stored before any send so a crash mid-dispatch resumes without
/// losing the plan.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RelayPlan {
    pub relays: Vec<RelayUrl>,
}

/// Retry policy. Default: AUTH-REQUIRED → reauth + 1 retry; transient →
/// up to 3 total attempts (initial + 2 retries) with exponential backoff
/// (1s before attempt 2, 4s before attempt 3). The 16s slot in the original
/// task spec is reachable by setting `transient_max_retries = 4`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetryPolicy {
    pub transient_max_retries: u32,
    pub auth_required_max_retries: u32,
    pub backoff_base_ms: u64,
    pub backoff_factor: u32,
    /// How long to wait for a relay `OK` before treating the relay as
    /// unresponsive and transitioning `InFlight → TimedOut`. A relay that
    /// accepts the TCP connection but silently drops the `EVENT` (never sends
    /// `OK` or closes the socket) would otherwise pin the publish forever.
    /// The `TimedOut` state feeds the existing retry ladder; after
    /// `transient_max_retries` the publish settles to `FailedAfterRetries`.
    /// Default: 30 000 ms (30 s).
    #[serde(default = "RetryPolicy::default_inflight_deadline_ms")]
    pub inflight_deadline_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            transient_max_retries: 3,
            auth_required_max_retries: 1,
            backoff_base_ms: 1_000,
            backoff_factor: 4,
            inflight_deadline_ms: Self::default_inflight_deadline_ms(),
        }
    }
}

impl RetryPolicy {
    fn default_inflight_deadline_ms() -> u64 {
        30_000
    }

    #[must_use] 
    pub fn backoff_for(&self, attempt_just_failed: u32) -> u64 {
        // attempt_just_failed is 1-indexed (the first send is attempt 1).
        // We want 1s after attempt 1, 4s after attempt 2, 16s after attempt 3.
        let mut delay = self.backoff_base_ms;
        for _ in 1..attempt_just_failed {
            delay = delay.saturating_mul(self.backoff_factor as u64);
        }
        delay
    }
}

/// Outcome of classifying an ack against the current state + policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetryVerdict {
    Settled(PerRelayState),
    ScheduleRetry { delay_ms: u64, next_attempt: u32 },
    Reauth { delay_ms: u64, next_attempt: u32 },
}

/// Pure transition function. Takes the current state + an ack + policy + a
/// `now_ms` clock reading and returns the next state plus an optional retry
/// directive. The engine is responsible for scheduling the retry; the state
/// machine never touches time except to record the timestamp into the state.
///
/// Classification of the raw ack into `AckClass` is performed here (the engine
/// is the only caller, and per D7 the dispatcher never sees `AckClass`).
pub fn apply_ack(
    state: &PerRelayState,
    ack: &RelayAck,
    policy: RetryPolicy,
    now_ms: u64,
) -> RetryVerdict {
    // Only InFlight states consume acks; everything else is a stale duplicate.
    if !matches!(state, PerRelayState::InFlight { .. }) {
        if state.is_terminal() {
            // Late-arriving ack for a state that already settled: hold the
            // settled state (idempotence per D7's capability contract).
            return RetryVerdict::Settled(state.clone());
        }
        // Ack arrived while we were Pending or already RelayError/TimedOut
        // (post-classification, pre-retry): treat as a stale duplicate.
        return RetryVerdict::Settled(state.clone());
    }
    let attempt = state.attempt().max(1);
    if ack.ok {
        return RetryVerdict::Settled(PerRelayState::Ok {
            acked_at_ms: now_ms,
        });
    }
    let message = ack.message.as_str();
    match classify_ack(ack) {
        AckClass::Permanent => RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
            reason: if message.is_empty() {
                ack.code.clone()
            } else {
                message.to_string()
            },
            last_at_ms: now_ms,
        }),
        AckClass::AuthRequired => {
            if attempt > policy.auth_required_max_retries {
                RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
                    reason: format!(
                        "auth-required after {} reauth attempts: {}",
                        attempt, message
                    ),
                    last_at_ms: now_ms,
                })
            } else {
                RetryVerdict::Reauth {
                    delay_ms: 0,
                    next_attempt: attempt + 1,
                }
            }
        }
        AckClass::Transient => {
            if attempt >= policy.transient_max_retries {
                let reason = if ack.code == TIMEOUT_CODE {
                    format!("timeout after {} retries", attempt)
                } else if message.is_empty() {
                    format!("transient after {} retries: {}", attempt, ack.code)
                } else {
                    format!("transient after {} retries: {}", attempt, message)
                };
                RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
                    reason,
                    last_at_ms: now_ms,
                })
            } else {
                RetryVerdict::ScheduleRetry {
                    delay_ms: policy.backoff_for(attempt),
                    next_attempt: attempt + 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Inline unit tests for the pure pieces of the per-relay state machine.
    //!
    //! Scope note: the integration-style coverage of `apply_ack` over an
    //! `InFlight` state (success / permanent / transient backoff ladder /
    //! reauth ladder) already lives in `publish/tests.rs`, and the
    //! code→`AckClass` mapping is exhaustively pinned there too. These tests
    //! deliberately cover the gaps that file leaves: `is_terminal`, `attempt`,
    //! `RetryPolicy` defaults + `backoff_for`, the non-`InFlight` stale-ack
    //! guard in `apply_ack`, plus a minimal `classify_ack` smoke set so the
    //! classifier is documented by test co-located with its definition.

    use super::*;

    // --- classify_ack: minimal smoke set (doc-by-test, not exhaustive) -----

    #[test]
    fn classify_ack_ok_is_permanent() {
        // ok=true never reaches the classifier via apply_ack, but the
        // classifier pins it to Permanent so an accidental call cannot
        // trigger a retry loop.
        assert_eq!(classify_ack(&RelayAck::ok("wss://relay.example/")), AckClass::Permanent);
    }

    #[test]
    fn classify_ack_permanent_codes_are_permanent() {
        // "duplicate" and "blocked"/"spam"-class rejections classify as
        // permanent — the engine gives up immediately, no retry.
        for code in ["duplicate", "blocked"] {
            let ack = RelayAck::failed("wss://relay.example/", code, "rejected");
            assert_eq!(classify_ack(&ack), AckClass::Permanent, "code={code}");
        }
    }

    #[test]
    fn classify_ack_auth_required_is_auth_required() {
        let ack = RelayAck::failed("wss://relay.example/", "auth-required", "please AUTH");
        assert_eq!(classify_ack(&ack), AckClass::AuthRequired);
    }

    #[test]
    fn classify_ack_unknown_code_is_transient() {
        // Unknown / unrecognised tokens fall through to the conservative
        // retry verdict. A timeout is the canonical transport-class case.
        let unknown = RelayAck::failed("wss://relay.example/", "totally-unknown-token", "huh");
        assert_eq!(classify_ack(&unknown), AckClass::Transient);
        assert_eq!(classify_ack(&RelayAck::timed_out("wss://relay.example/")), AckClass::Transient);
    }

    // --- PerRelayState::is_terminal -----------------------------------------

    #[test]
    fn is_terminal_true_only_for_ok_and_failed_after_retries() {
        assert!(PerRelayState::Ok { acked_at_ms: 10 }.is_terminal());
        assert!(PerRelayState::FailedAfterRetries {
            reason: "gave up".into(),
            last_at_ms: 10,
        }
        .is_terminal());
    }

    #[test]
    fn is_terminal_false_for_non_settled_states() {
        let non_terminal = [
            PerRelayState::Pending,
            PerRelayState::InFlight { sent_at_ms: 1, attempt: 1 },
            PerRelayState::RelayError {
                message: "transient".into(),
                attempt: 1,
                last_at_ms: 1,
            },
            PerRelayState::TimedOut { attempt: 1, last_at_ms: 1 },
        ];
        for state in non_terminal {
            assert!(!state.is_terminal(), "expected non-terminal: {state:?}");
        }
    }

    // --- PerRelayState::attempt ---------------------------------------------

    #[test]
    fn attempt_reports_zero_for_pending_and_settled_states() {
        assert_eq!(PerRelayState::Pending.attempt(), 0);
        assert_eq!(PerRelayState::Ok { acked_at_ms: 5 }.attempt(), 0);
        assert_eq!(
            PerRelayState::FailedAfterRetries { reason: "x".into(), last_at_ms: 5 }.attempt(),
            0
        );
    }

    #[test]
    fn attempt_reports_inner_counter_for_in_progress_states() {
        assert_eq!(PerRelayState::InFlight { sent_at_ms: 1, attempt: 2 }.attempt(), 2);
        assert_eq!(
            PerRelayState::RelayError { message: "e".into(), attempt: 3, last_at_ms: 1 }.attempt(),
            3
        );
        assert_eq!(PerRelayState::TimedOut { attempt: 4, last_at_ms: 1 }.attempt(), 4);
    }

    // --- RetryPolicy ---------------------------------------------------------

    #[test]
    fn retry_policy_default_matches_documented_values() {
        let p = RetryPolicy::default();
        assert_eq!(p.transient_max_retries, 3);
        assert_eq!(p.auth_required_max_retries, 1);
        assert_eq!(p.backoff_base_ms, 1_000);
        assert_eq!(p.backoff_factor, 4);
    }

    #[test]
    fn backoff_for_follows_documented_exponential_ladder() {
        // Docstring: 1s after attempt 1, 4s after attempt 2, 16s after 3.
        let p = RetryPolicy::default();
        assert_eq!(p.backoff_for(1), 1_000);
        assert_eq!(p.backoff_for(2), 4_000);
        assert_eq!(p.backoff_for(3), 16_000);
    }

    #[test]
    fn backoff_for_saturates_instead_of_overflowing() {
        // A pathological attempt count must not panic on multiply overflow.
        let p = RetryPolicy::default();
        assert_eq!(p.backoff_for(1_000), u64::MAX);
    }

    // --- apply_ack: non-InFlight stale-ack guard ----------------------------

    #[test]
    fn apply_ack_on_terminal_ok_holds_settled_state_idempotently() {
        // A late ack for an already-settled relay must not mutate it.
        let settled = PerRelayState::Ok { acked_at_ms: 1_000 };
        let verdict = apply_ack(
            &settled,
            &RelayAck::ok("wss://relay.example/"),
            RetryPolicy::default(),
            9_999,
        );
        assert_eq!(verdict, RetryVerdict::Settled(settled));
    }

    #[test]
    fn apply_ack_on_terminal_failure_holds_settled_state_idempotently() {
        let settled = PerRelayState::FailedAfterRetries {
            reason: "blocked: spam".into(),
            last_at_ms: 1_000,
        };
        let verdict = apply_ack(
            &settled,
            &RelayAck::failed("wss://relay.example/", "blocked", "blocked: spam"),
            RetryPolicy::default(),
            9_999,
        );
        assert_eq!(verdict, RetryVerdict::Settled(settled));
    }

    #[test]
    fn apply_ack_on_pending_treats_ack_as_stale_duplicate() {
        // An ack arriving before the send was recorded as InFlight is stale;
        // the state is held unchanged rather than advanced.
        let verdict = apply_ack(
            &PerRelayState::Pending,
            &RelayAck::ok("wss://relay.example/"),
            RetryPolicy::default(),
            5_000,
        );
        assert_eq!(verdict, RetryVerdict::Settled(PerRelayState::Pending));
    }
}
