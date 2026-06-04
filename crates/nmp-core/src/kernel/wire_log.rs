//! Structured telemetry for the relay claim-expansion search path.
//!
//! # Gate
//!
//! All output is suppressed unless the `NMP_CLAIM_LOG` environment variable is
//! set (any value). The check is cached after the first call via
//! [`OnceLock`](std::sync::OnceLock) — one atomic load per emit, no OS
//! syscall on the hot path.
//!
//! # Why `NMP_CLAIM_LOG` not `NMP_WIRE_LOG`
//!
//! `NMP_WIRE_LOG` is already used by `nmp-network::relay_worker::socket_io`
//! as a *file-path* raw-frame logger (`[ts] <relay> → <text>\n`). The two
//! semantics are incompatible, so this layer uses a distinct name.
//!
//! # Call-site instrumentation
//!
//! Call sites wired by W8b (all env-gated via `NMP_CLAIM_LOG`):
//!
//! | Event              | File                        | Function                          |
//! |--------------------|-----------------------------|------------------------------------|
//! | `ReqEmit`          | `kernel/requests/mod.rs`    | `register_planner_wire_frames`     |
//! | `EventRx`          | `kernel/relay_score_record.rs` | `record_claim_expansion_hit`    |
//! | `EoseRx{matched:true}`  | `kernel/relay_score_record.rs` | `record_claim_expansion_eose_no_match` (early-return branch) |
//! | `EoseRx{matched:false}` | `kernel/claim_expansion.rs` | `on_claim_outcome_eose_no_match` |
//! | `ClaimPhaseAdvance` | `kernel/claim_expansion.rs` + `claim_expansion_helpers.rs` | register / advance / terminate |
//! | `ScoreUpdate`      | `kernel/relay_score_record.rs` | `record_claim_outcome`          |
//!
//! # D6 — panic safety
//!
//! [`serde_json::to_string`] failures are absorbed by
//! [`unwrap_or_default`](Result::unwrap_or_default): a serialization failure
//! produces an empty string rather than a panic.

use std::io::{self, Write as IoWrite};

/// Structured events emitted to stderr when `NMP_CLAIM_LOG` is set.
///
/// Each variant maps to one logical event in the claim-expansion search path.
/// The `#[serde(tag = "type")]` attribute adds a discriminant field (`"type":
/// "ReqEmit"`, etc.) so grep-based acceptance tests can filter by event kind.
#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub(crate) enum WireLogEvent<'a> {
    /// A subscription request was emitted to a relay.
    ReqEmit {
        sub_id: &'a str,
        relay_url: &'a str,
        /// One of `"phase1"`, `"phase2"`, `"claim"`, `"discovery"`.
        phase: &'a str,
        /// Hex-encoded author pubkey.
        author: &'a str,
        /// Whether an outbox hint was available that influenced relay selection.
        has_hint: bool,
    },
    /// An EOSE frame was received from a relay.
    EoseRx {
        sub_id: &'a str,
        relay_url: &'a str,
        /// `true` if at least one matching event was received before this EOSE.
        matched: bool,
    },
    /// An event frame was received from a relay.
    EventRx {
        sub_id: &'a str,
        relay_url: &'a str,
        event_id: &'a str,
        author: &'a str,
    },
    /// The claim state machine advanced to a new phase.
    ClaimPhaseAdvance {
        author: &'a str,
        from: &'a str,
        to: &'a str,
        /// Human-readable reason (e.g. `"phase1_miss"`, `"eose_no_match"`).
        reason: &'a str,
    },
    /// The relay score record was updated for an author/relay pair.
    ScoreUpdate {
        author: &'a str,
        relay_url: &'a str,
        /// Delta description (e.g. `"+3"`, `"-1"`).
        delta: &'a str,
        new_weight: f32,
    },
}

/// Emit a structured claim-log line to stderr if `NMP_CLAIM_LOG` is set.
///
/// Checks the gate via one atomic load (see [`claim_log_enabled`]) then
/// delegates to [`write_wire_line`]. No allocation occurs when the gate is
/// closed.
pub(crate) fn log_wire(event: WireLogEvent<'_>) {
    write_wire_line(&mut io::stderr().lock(), claim_log_enabled(), &event);
}

/// Inner writer extracted for testability.
///
/// Writes `"nmp.wire <json>\n"` to `w` only when `enabled` is `true`.
/// Tests drive this directly (with a `Vec<u8>` sink and an explicit
/// `enabled` flag) so they exercise the gate decision without touching
/// the `OnceLock` or env-var state. This removes the vacuous-gate trap
/// identified in the W8a codex review.
///
/// Call sites outside this module must go through [`log_wire`], which
/// supplies the production gate value, so the env check cannot be
/// accidentally bypassed.
///
/// # D6 — panic safety
/// JSON encoding errors produce an empty payload string (`""`) rather than
/// panicking.
pub(super) fn write_wire_line<W: IoWrite>(w: &mut W, enabled: bool, event: &WireLogEvent<'_>) {
    if !enabled {
        return;
    }
    let payload = serde_json::to_string(event).unwrap_or_default();
    // `writeln!` failure is intentionally discarded (e.g. broken pipe during
    // test teardown); the fallible return is not meaningful here.
    let _ = writeln!(w, "nmp.wire {payload}");
}

/// Returns `true` if the `NMP_CLAIM_LOG` environment variable is set.
///
/// The result is cached after the first call in a
/// [`OnceLock`](std::sync::OnceLock). Subsequent calls are a single atomic
/// load — no OS syscall on the hot ingest path (§8.8, R5).
///
/// **Consequence**: setting `NMP_CLAIM_LOG` *after* the first call to any
/// function that transitively calls this one will have no effect. This matches
/// the convention for other env-gated diagnostic flags in this codebase.
fn claim_log_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NMP_CLAIM_LOG").is_some())
}
