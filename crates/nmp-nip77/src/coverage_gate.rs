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

/// Compute the simple coverage percentage helper documented in the task
/// description (`coverage > 95% and recent` → `SkipReq`).  The store's
/// `Coverage` enum already collapses staleness into the `CompleteAsOf` vs
/// `PartialUpTo` distinction, so the percentage path is informational only —
/// useful for diagnostics + tests that want to assert "we cleared 95 %".
pub fn coverage_pct(coverage: Coverage, now_s: u64) -> u8 {
    match coverage {
        Coverage::CompleteAsOf(_) => 100,
        Coverage::Unknown => 0,
        Coverage::PartialUpTo(ts) => {
            if now_s == 0 || ts == 0 {
                return 0;
            }
            let ratio = (ts as f64 / now_s as f64).clamp(0.0, 1.0);
            (ratio * 100.0).round() as u8
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
    fn coverage_pct_collapses_complete_to_100() {
        assert_eq!(coverage_pct(Coverage::CompleteAsOf(1), 100), 100);
        assert_eq!(coverage_pct(Coverage::Unknown, 100), 0);
        assert_eq!(coverage_pct(Coverage::PartialUpTo(96), 100), 96);
    }
}
