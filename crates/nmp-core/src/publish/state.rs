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
/// Per D7 (capabilities report), the dispatcher hands the engine raw OK / NOTICE
/// / disconnect data; the engine classifies it into transient vs persistent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum RelayAck {
    Ok {
        relay_url: RelayUrl,
    },
    Failed {
        relay_url: RelayUrl,
        message: String,
        class: AckClass,
    },
    TimedOut {
        relay_url: RelayUrl,
    },
}

/// Classification the dispatcher attaches to a failure. Kept narrow so the
/// engine's retry policy is reproducible across platforms (per D7: policy is
/// Rust's; capabilities are reports).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AckClass {
    /// `AUTH-REQUIRED` — re-auth via the active signer, retry once.
    AuthRequired,
    /// Connection drop, socket reset, transient I/O — retry with backoff.
    Transient,
    /// `invalid:`, `pow:`, `restricted:`, `blocked:` — permanent rejection;
    /// do not retry, surface to the snapshot.
    Permanent,
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
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Ok { .. } | Self::FailedAfterRetries { .. })
    }

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
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            transient_max_retries: 3,
            auth_required_max_retries: 1,
            backoff_base_ms: 1_000,
            backoff_factor: 4,
        }
    }
}

impl RetryPolicy {
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
pub fn apply_ack(
    state: &PerRelayState,
    ack: &RelayAck,
    policy: RetryPolicy,
    now_ms: u64,
) -> RetryVerdict {
    let attempt = state.attempt().max(1);
    match (state, ack) {
        (PerRelayState::InFlight { .. }, RelayAck::Ok { .. }) => {
            RetryVerdict::Settled(PerRelayState::Ok {
                acked_at_ms: now_ms,
            })
        }
        (
            PerRelayState::InFlight { .. },
            RelayAck::Failed {
                message,
                class: AckClass::Permanent,
                ..
            },
        ) => RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
            reason: message.clone(),
            last_at_ms: now_ms,
        }),
        (
            PerRelayState::InFlight { .. },
            RelayAck::Failed {
                message,
                class: AckClass::AuthRequired,
                ..
            },
        ) => {
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
        (
            PerRelayState::InFlight { .. },
            RelayAck::Failed {
                message,
                class: AckClass::Transient,
                ..
            },
        ) => {
            if attempt >= policy.transient_max_retries {
                RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
                    reason: format!("transient after {} retries: {}", attempt, message),
                    last_at_ms: now_ms,
                })
            } else {
                RetryVerdict::ScheduleRetry {
                    delay_ms: policy.backoff_for(attempt),
                    next_attempt: attempt + 1,
                }
            }
        }
        (PerRelayState::InFlight { .. }, RelayAck::TimedOut { .. }) => {
            if attempt >= policy.transient_max_retries {
                RetryVerdict::Settled(PerRelayState::FailedAfterRetries {
                    reason: format!("timeout after {} retries", attempt),
                    last_at_ms: now_ms,
                })
            } else {
                RetryVerdict::ScheduleRetry {
                    delay_ms: policy.backoff_for(attempt),
                    next_attempt: attempt + 1,
                }
            }
        }
        // Late-arriving ack for a state that already settled: hold the
        // settled state (idempotence per D7's capability contract).
        (settled, _) if settled.is_terminal() => RetryVerdict::Settled(settled.clone()),
        // Ack arrived while we were Pending or already RelayError/TimedOut
        // (post-classification, pre-retry): treat as a stale duplicate.
        (state, _) => RetryVerdict::Settled(state.clone()),
    }
}
