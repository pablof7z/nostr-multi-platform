//! Robustness family 5 — GC COVERAGE-HOLE SOUNDNESS (deeper than "RSS is flat").
//!
//! Hypothesis: with LRU eviction EXPLICITLY enabled (a `GcBudget` with a durable
//! ceiling — production default is `usize::MAX`, so eviction is effectively off
//! and the oracle must opt in), eviction MUST NEVER strand an event that an
//! active interest still needs (the coverage-ledger pin concern). "Bounded but
//! wrong" is the dangerous case the RSS gates miss.
//!
//! STATUS: BLOCKED on master — this oracle needs TWO seams that do not exist on
//! the production/test FFI surface today. Per the honesty contract we emit a
//! BLOCKED row naming the exact missing seams + the minimal, non-churning way to
//! wire them, rather than a diluted or faked pass.

use crate::config::{Args, Phase};
use crate::report::{GateRow, SanityReport, Verdict};

pub fn run_gc_soundness(report: &mut SanityReport, _args: &Args) {
    let phase = Phase::GcSoundness.as_str();

    // Seam 1: opt-in to a bounded GcBudget. Production default is usize::MAX
    // (crates/nmp-store/src/types/gc.rs), so without a way to configure a
    // durable LRU ceiling at app construction, no eviction ever occurs and the
    // coverage-hole invariant cannot be exercised at all.
    report.push(GateRow::unmeasured(
        "gc-lru-opt-in",
        phase,
        "(none)",
        "GcBudget durable ceiling configuration (nmp-store/src/types/gc.rs)",
        "LRU eviction enabled with a bounded ceiling",
        Verdict::Blocked,
        "HOOK MISSING — no FFI/config seam to construct an NmpApp with a bounded GcBudget \
         (production default is usize::MAX = eviction off). Minimal wiring: a test-support-gated \
         `nmp_app_configure_gc_budget(app, max_events)` that sets the budget before nmp_app_start, \
         mirroring nmp_app_set_storage_path. Do NOT change the production default.",
    ));

    // Seam 2: read back which events an active interest still needs vs which the
    // LRU evicted, to assert the intersection is empty (no stranded coverage).
    report.push(GateRow::unmeasured(
        "gc-no-stranded-coverage",
        phase,
        "(none)",
        "kernel::ram_eviction::RamEvictionReport ∩ active-interest coverage ledger",
        "evicted-set ∩ still-needed-set == ∅",
        Verdict::Blocked,
        "HOOK MISSING — RamEvictionReport is internal to run_gc_step (no FFI read seam), and the \
         active-interest coverage ledger (ADR-0056 K3) is not exposed either. Minimal wiring: a \
         test-support-gated `nmp_app_read_ram_eviction_stats` counter (evicted ids) sibling to \
         nmp_app_read_projection_churn_stats, plus a read of the coverage-ledger pin set. The \
         assertion is `evicted ∩ pinned == ∅`. Do NOT scrape run_gc_step internals.",
    ));

    report.finding(
        "BLOCKED (family 5) — GC coverage-hole soundness needs two test-support read/config seams \
         that don't exist on master: (1) nmp_app_configure_gc_budget (opt-in bounded LRU), and \
         (2) nmp_app_read_ram_eviction_stats + coverage-ledger pin read. Both are read-only/config \
         accessors that can be added WITHOUT touching the churning store internals. Until then this \
         oracle is honestly BLOCKED, not faked.",
    );
}
