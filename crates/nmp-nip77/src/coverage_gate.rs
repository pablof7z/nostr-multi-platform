//! Coverage gate — translates a [`Coverage`] reading + capability cache into
//! the [`SyncStrategy`] the planner should execute.
//!
//! This is the load-bearing decision point for **D2 — negentropy first, REQ
//! second**.  The planner calls [`decide_strategy`] before issuing any REQ;
//! the returned strategy answers three questions in one shot:
//!
//! 1. Is the local cache authoritative (`SkipReq`)?
//! 2. Should we sync first and fall back to REQ on gap (`NegThenReq`)?
//! 3. Are we forced to REQ from a known watermark (`ReqSince`)?
//!
//! ## Threshold
//!
//! The task description fixes the cutover at **95 %** coverage and *recent*
//! freshness.  Freshness comes from the store's `Coverage::CompleteAsOf`
//! signal, which already encodes the 300-second staleness window from
//! `docs/design/lmdb/watermarks.md` §1.1.  We never re-implement that check
//! here.
//!
//! ## D6
//!
//! [`decide_strategy`] is infallible — every input maps to a strategy.  No
//! error type, no `Result`.  The planner never has a reason to surface a
//! "coverage decision failed" toast.

use nmp_core::store::{Coverage, WatermarkKey, WatermarkRow};

use crate::capability::RelayCapabilities;

/// Coverage proportion threshold above which a `CompleteAsOf` row earns
/// `SyncStrategy::SkipReq`.  Documented at 95 % in the M4 task spec; kept
/// public so callers and tests don't hard-code the constant in their own
/// asserts.
pub const COVERAGE_THRESHOLD_PCT: u8 = 95;

/// Inputs the gate consumes.
#[derive(Clone, Debug)]
pub struct GateInputs {
    pub coverage: Coverage,
    /// `None` when the capability probe has not run; the gate is conservative
    /// and treats unknown capability as "do not negentropy yet".
    pub capabilities: Option<RelayCapabilities>,
    /// The previously-persisted watermark, if any.  Used to seed `ReqSince`.
    pub watermark: Option<WatermarkRow>,
}

/// What the planner should do next for a given `(filter, relay)` pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncStrategy {
    /// Cache is authoritative; emit no REQ.
    SkipReq,
    /// Run a negentropy reconciliation; if anything comes back in `need`, the
    /// planner converts that into an `ids`-scoped REQ for the missing events.
    NegThenReq,
    /// Negentropy not available — REQ from `since` (the watermark's
    /// `synced_up_to + 1`, or `0` if no watermark).  Per the M4 doc-typo
    /// noted in the task description (`last_seen` ≡ `synced_up_to`), the
    /// `+1` keeps us from re-fetching the boundary event.
    ReqSince(u64),
    /// Optional resume blob the planner can hand back to
    /// [`crate::Reconciler::resume_client`] when starting the next sync.
    /// Carried alongside `NegThenReq` and `ReqSince` so the planner doesn't
    /// have to re-read the watermark separately.
    Resume {
        next: Box<SyncStrategy>,
        state: Vec<u8>,
    },
}

impl SyncStrategy {
    /// Convenience: discard any `Resume` wrapper.
    pub fn inner(&self) -> &SyncStrategy {
        match self {
            SyncStrategy::Resume { next, .. } => next.inner(),
            other => other,
        }
    }

    /// `true` iff the strategy results in any wire traffic.
    pub fn issues_wire_traffic(&self) -> bool {
        !matches!(self.inner(), SyncStrategy::SkipReq)
    }
}

/// Apply the gate.
pub fn decide_strategy(_key: &WatermarkKey, inputs: GateInputs) -> SyncStrategy {
    let GateInputs {
        coverage,
        capabilities,
        watermark,
    } = inputs;

    // Authoritative cache miss — D2 explicitly forbids us from REQ-ing here.
    if matches!(coverage, Coverage::CompleteAsOf(_)) {
        return SyncStrategy::SkipReq;
    }

    let prefer_neg = capabilities
        .map(|c| c.supports_nip77)
        .unwrap_or(false);

    let base = if prefer_neg {
        SyncStrategy::NegThenReq
    } else {
        let since = watermark
            .as_ref()
            .map(|w| w.synced_up_to.saturating_add(1))
            .unwrap_or(0);
        SyncStrategy::ReqSince(since)
    };

    match watermark.and_then(|w| w.last_negentropy_state) {
        Some(state) if prefer_neg => SyncStrategy::Resume {
            next: Box::new(base),
            state,
        },
        _ => base,
    }
}

/// Recency of a `(filter, relay)` watermark, normalised to `[0.0, 1.0]`.
///
/// This is **not** an event-count coverage ratio (we never claimed to know how
/// many events the relay actually holds). It is a *recency* signal — the
/// fraction of wall-clock time, between the unix epoch and `now_s`, that the
/// watermark covers. `CompleteAsOf` collapses to `1.0` because that variant
/// already encodes the "fresh enough to be authoritative" decision made by
/// the store's staleness window.
///
/// | Coverage variant     | Returned ratio                          |
/// |----------------------|-----------------------------------------|
/// | `CompleteAsOf(_)`    | `1.0` (cache is authoritative)          |
/// | `PartialUpTo(ts)`    | `ts / now_s`, clamped to `[0.0, 1.0]`   |
/// | `Unknown`            | `0.0` (no signal)                       |
///
/// **Use cases.** Diagnostics surfaces (ADR-0007 wire view), firehose-bench
/// instrumentation, and tests that assert a watermark crossed a freshness
/// threshold. The planner gate itself never calls this — it consumes the
/// stronger `Coverage::CompleteAsOf` signal directly via `decide_strategy`.
///
/// The previous `coverage_pct` name and `u8` percentage return type
/// misleadingly suggested this number measured cache completeness; it never
/// did. Renamed in the T53 follow-up per the M4 codex review at
/// `docs/perf/codex-reviews/076173d.md` (P3 misleading public helper).
pub fn freshness_ratio(coverage: Coverage, now_s: u64) -> f32 {
    match coverage {
        Coverage::CompleteAsOf(_) => 1.0,
        Coverage::Unknown => 0.0,
        Coverage::PartialUpTo(ts) => {
            if now_s == 0 || ts == 0 {
                return 0.0;
            }
            ((ts as f64 / now_s as f64).clamp(0.0, 1.0)) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::SyncMethod;

    fn key() -> WatermarkKey {
        WatermarkKey {
            filter_hash: [0; 32],
            relay_url: "wss://r/".into(),
        }
    }

    fn watermark(synced_up_to: u64, state: Option<Vec<u8>>) -> WatermarkRow {
        WatermarkRow {
            key: key(),
            synced_up_to,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: state,
            bytes_saved_vs_req: 0,
            updated_at: synced_up_to,
        }
    }

    #[test]
    fn complete_coverage_skips_req() {
        let s = decide_strategy(
            &key(),
            GateInputs {
                coverage: Coverage::CompleteAsOf(100),
                capabilities: Some(RelayCapabilities {
                    supports_nip77: true,
                }),
                watermark: Some(watermark(100, None)),
            },
        );
        assert_eq!(s, SyncStrategy::SkipReq);
        assert!(!s.issues_wire_traffic());
    }

    #[test]
    fn partial_coverage_with_nip77_runs_neg() {
        let s = decide_strategy(
            &key(),
            GateInputs {
                coverage: Coverage::PartialUpTo(50),
                capabilities: Some(RelayCapabilities {
                    supports_nip77: true,
                }),
                watermark: Some(watermark(50, None)),
            },
        );
        assert_eq!(s.inner(), &SyncStrategy::NegThenReq);
    }

    #[test]
    fn partial_coverage_without_nip77_falls_back_to_req_since() {
        let s = decide_strategy(
            &key(),
            GateInputs {
                coverage: Coverage::PartialUpTo(50),
                capabilities: Some(RelayCapabilities {
                    supports_nip77: false,
                }),
                watermark: Some(watermark(50, None)),
            },
        );
        assert_eq!(s, SyncStrategy::ReqSince(51));
    }

    #[test]
    fn unknown_coverage_with_no_watermark_req_from_zero() {
        let s = decide_strategy(
            &key(),
            GateInputs {
                coverage: Coverage::Unknown,
                capabilities: None,
                watermark: None,
            },
        );
        assert_eq!(s, SyncStrategy::ReqSince(0));
    }

    #[test]
    fn resume_blob_propagates_when_neg_preferred() {
        let blob = vec![1u8, 2, 3];
        let s = decide_strategy(
            &key(),
            GateInputs {
                coverage: Coverage::PartialUpTo(50),
                capabilities: Some(RelayCapabilities {
                    supports_nip77: true,
                }),
                watermark: Some(watermark(50, Some(blob.clone()))),
            },
        );
        match s {
            SyncStrategy::Resume { next, state } => {
                assert_eq!(*next, SyncStrategy::NegThenReq);
                assert_eq!(state, blob);
            }
            other => panic!("expected Resume, got {other:?}"),
        }
    }

    #[test]
    fn freshness_ratio_collapses_complete_to_one() {
        assert_eq!(freshness_ratio(Coverage::CompleteAsOf(1), 100), 1.0);
        assert_eq!(freshness_ratio(Coverage::Unknown, 100), 0.0);
        // PartialUpTo: 96 of "now=100" seconds → 0.96 recency.
        let r = freshness_ratio(Coverage::PartialUpTo(96), 100);
        assert!((r - 0.96).abs() < 1e-5, "expected ~0.96, got {r}");
    }

    #[test]
    fn freshness_ratio_zero_guard_on_zero_now_or_zero_ts() {
        // P3 corrected semantics — every degenerate input maps to 0.0, never
        // panics or returns a >1.0 ratio.
        assert_eq!(freshness_ratio(Coverage::PartialUpTo(100), 0), 0.0);
        assert_eq!(freshness_ratio(Coverage::PartialUpTo(0), 100), 0.0);
    }

    #[test]
    fn freshness_ratio_clamps_overshoot() {
        // P3 regression — a misconfigured watermark (`ts > now`) must not
        // report freshness > 1.0; the `CompleteAsOf` variant is the only
        // path that earns the `1.0` authoritative claim.
        let r = freshness_ratio(Coverage::PartialUpTo(200), 100);
        assert_eq!(r, 1.0, "overshoot must clamp to 1.0, not {r}");
    }
}
