//! Planner-side coverage gate.
//!
//! Bridges [`crate::coverage_gate::decide_strategy`] into the M2 subscription
//! planner.  The planner produces a [`CompiledPlan`]; this gate rewrites it
//! per `(filter, relay)` according to the current watermark + capability
//! reading:
//!
//! | Strategy | Plan rewrite |
//! |---|---|
//! | `SkipReq` | Sub-shape dropped from the relay plan; relay's plan removed if empty. |
//! | `ReqSince(s)` | Sub-shape's `since` is bumped to `s` (no-op if existing `since` is already ≥ `s`). |
//! | `NegThenReq` | Sub-shape preserved; the planner emits the REQ unchanged.  The companion negentropy run happens on the parallel sync path. |
//! | `Resume { next, .. }` | Same as `next`; the resume blob is consumed by the negentropy run, not the planner. |
//!
//! Returns a [`CoverageReport`] enumerating the decisions taken so callers
//! (e.g. ADR-0007 diagnostics, the firehose bench) can audit the gate's
//! behaviour without re-running it.
//!
//! ## Doctrine
//!
//! * **D2** — every relay plan first consults coverage before its REQ flies.
//! * **D6** — the gate is infallible: every input maps to a deterministic
//!   plan rewrite.  No `Result`.  No FFI surface to leak through.

use std::collections::BTreeMap;

use nmp_core::planner::CompiledPlan;
use nmp_core::store::{EventStore, WatermarkKey};

use crate::capability::CapabilityCache;
use crate::coverage_gate::{decide_strategy, GateInputs, SyncStrategy};

/// Per-`(filter, relay)` outcome of the gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateDecision {
    Skipped,
    BumpedSince(u64),
    Kept,
}

/// Aggregate audit trail.
#[derive(Clone, Debug, Default)]
pub struct CoverageReport {
    pub decisions: BTreeMap<(String, String), GateDecision>,
}

impl CoverageReport {
    pub fn count_skipped(&self) -> usize {
        self.decisions
            .values()
            .filter(|d| matches!(d, GateDecision::Skipped))
            .count()
    }

    pub fn count_bumped(&self) -> usize {
        self.decisions
            .values()
            .filter(|d| matches!(d, GateDecision::BumpedSince(_)))
            .count()
    }
}

/// Rewrite `plan` in place per the M4 coverage gate.
///
/// `canonicalise` maps a sub-shape's filter to the 32-byte canonical filter
/// hash expected by [`WatermarkKey::filter_hash`].  The kernel will eventually
/// expose `canonical_filter_hash(&Filter)` (see `docs/design/lmdb/watermarks.md`
/// §3); until it does, callers pass their own hash function — keeping
/// `nmp-nip77` decoupled from the (still-design-stage) canonical encoder.
///
/// INVARIANT (D2 — negentropy-first): this function IS the D2 coverage gate.
/// Every `CompiledPlan` MUST pass through it before any wire REQ is emitted
/// from that plan, so that authoritative `(filter, relay)` pairs are skipped
/// and `since` is bumped before the REQ flies. The constraint is NOT enforced
/// by the type system: `apply_coverage_filter` mutates `&mut CompiledPlan` in
/// place rather than producing a distinct gated type, and `CompiledPlan` lives
/// in `nmp-core` where the wire-emitter (`subs::wire::plan_diff`) can consume
/// it directly without proof the gate ran. D2 is therefore convention-only,
/// enforced by code review. The production kernel does NOT currently install
/// this gate at all — see the `TODO(D2)` on `PlanCoverageHook` in
/// `nmp-core::subs` (`crates/nmp-core/src/subs/mod.rs`) and its open tracking
/// sentinel `d2_production_kernel_installs_coverage_hook`.
pub fn apply_coverage_filter<F>(
    plan: &mut CompiledPlan,
    store: &dyn EventStore,
    caps: &dyn CapabilityCache,
    canonicalise: F,
) -> CoverageReport
where
    F: Fn(&nmp_core::planner::SubShape) -> [u8; 32],
{
    let mut report = CoverageReport::default();
    let mut empty_relays: Vec<String> = Vec::new();

    for (relay_url, relay_plan) in plan.per_relay.iter_mut() {
        relay_plan.sub_shapes.retain_mut(|sub| {
            let filter_hash = canonicalise(sub);
            let key = WatermarkKey {
                filter_hash,
                relay_url: relay_url.clone(),
            };
            let coverage = store.coverage(&key).unwrap_or(nmp_core::store::Coverage::Unknown);
            let watermark = store.read_watermark(&key).ok().flatten();
            let capabilities = caps.get(relay_url);
            let strategy = decide_strategy(
                &key,
                GateInputs {
                    coverage,
                    capabilities,
                    watermark,
                },
            );
            let (keep, decision) = apply_strategy_to_sub(sub, &strategy);
            report.decisions.insert(
                (relay_url.clone(), hex32(&filter_hash)),
                decision,
            );
            keep
        });
        if relay_plan.sub_shapes.is_empty() {
            empty_relays.push(relay_url.clone());
        }
    }
    for relay_url in empty_relays {
        plan.per_relay.remove(&relay_url);
    }
    report
}

fn apply_strategy_to_sub(
    sub: &mut nmp_core::planner::SubShape,
    strategy: &SyncStrategy,
) -> (bool, GateDecision) {
    match strategy.inner() {
        SyncStrategy::SkipReq => (false, GateDecision::Skipped),
        SyncStrategy::ReqSince(since) => {
            if sub.shape.since.map(|s| s < *since).unwrap_or(true) {
                sub.shape.since = Some(*since);
                // Mutating `shape.since` changes wire identity. The
                // wire-emitter keys sub-ids by `canonical_filter_hash`
                // (`subs::wire::sub_id_for`), so we must recompute it here —
                // otherwise the diff against the prior plan would see no
                // change and the new `since` would never reach the relay.
                // See the M4 codex review at
                // `docs/perf/codex-reviews/076173d.md` (P1 plan-identity).
                sub.recompute_hash();
                (true, GateDecision::BumpedSince(*since))
            } else {
                (true, GateDecision::Kept)
            }
        }
        SyncStrategy::NegThenReq => (true, GateDecision::Kept),
        // `Resume` is collapsed by `.inner()`, so it's unreachable here.
        SyncStrategy::Resume { .. } => (true, GateDecision::Kept),
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

// Unit tests live in `planner_gate_tests.rs` (sibling file) so this module
// stays under the 300 LOC soft cap.  Registered from `lib.rs`.

