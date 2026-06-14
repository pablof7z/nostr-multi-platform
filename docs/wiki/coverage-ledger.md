---
title: Coverage Ledger
slug: coverage-ledger
topic: event-acquisition
summary: The coverage ledger (K3) discharges P2 wholesale and is the precondition #1090's eviction re-enable was always missing; it is the surgery and goes last, behind
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Coverage Ledger

## Placement and Sequencing

The coverage ledger (K3) discharges P2 wholesale and is the precondition #1090's eviction re-enable was always missing; it is the surgery and goes last, behind two cheap soundness restorations that de-risk it. The 30-day call prioritizes K1 through gift-unwrap (the only change two reviewers converged on independently), K2 through the global-hook slots, and the sync-soundness pair (un-floored NEG-OPEN + slot-lifetime cache-serve marker). LRU recency stamps on get_by_id reads are deferred (buffered in memory on an existing AtomicU64, flushed once per 60s GC pass) instead of writing per-read transactions, eliminating the point-read write txn while staying D7-clean.

<!-- citations: [^2e544-51] [^2e544-346] [^2e544-364] [^2e544-387] [^2e544-461] -->
## Oracles

The convergence property test (P2-d) and fixture-relay journey (follow-after-thread-reply backfills the author's history) are the oracles for the coverage ledger. The coverage-ledger read swap is gated by a fixture-relay journey test proving the H1 backfill-suppression bug (follow-after-thread-reply) is fixed flag-on and still-broken flag-off.

<!-- citations: [^2e544-52] [^2e544-365] [^2e544-460] -->
## Un-flooring NEG-OPEN (K3 Rung 2.1)

Un-flooring NEG-OPEN makes findings 1-3 self-healing for all NEG-eligible shapes without touching watermark code, because reconciliation covers the full window instead of inheriting the presence-derived floor. K3 Stage A (ADR-0056 + NEG-OPEN un-floor) is merged as PRs #1371 and #1372.

<!-- citations: [^2e544-53] [^2e544-366] [^2e544-388] -->
## Sync One-Change Fix

The fix is inverting the composition order: when the NIP-77 interceptor claims a frame, reconcile the un-floored window, keeping the floor only on plain REQs. <!-- [^2e544-54] -->

## Disabled Guards

The persisted claims sub-db (ClaimerId, OverPinned) must be deleted; gc_step receives a derived pin set from the kernel instead. The GC honest-budget Phase-3 hourly gate and the HOT_EVENT_CEILING are disabled until store-claims are wired (tracked in #1090), with the cursor livelock edge case tracked in #1097. LRU eviction must lower the coverage watermark floor in the same transaction when deleting events below it, and an eviction floor rule must be solved in the same change that re-enables the ceiling.

<!-- citations: [^da6b1-68] [^2e544-447] [^2e544-462] -->
## Coverage Ledger as Sole Since-Floor Source

The coverage ledger (WatermarkRow) is the sole source of since-floors, written by the sync engine at EOSE/NEG-DONE per (filter_hash, relay), replacing the presence-derived heuristic that caused permanent backfill gaps. When the coverage ledger flag is on and no coverage row exists for a shape, the floor is refused (full window REQ) rather than falling back to the unsound presence value. Eviction that deletes an event below a covered_through watermark lowers that row to oldest_evicted - 1 (or clears it) in the same transaction/lock as the delete, on both Mem and LMDB backends. A CoverageGuard (opaque matches closure + the row key) is passed from the kernel into the store GC method, keeping the shape-match predicate in the kernel (D0) while making the watermark lowering atomic with the eviction. The address-pointer watermark branch uses the same min/abort rule as the authors branch: takes min over coords and returns None (no floor) when any coord has zero stored events. NEG-OPEN carries the shape's original un-floored window (or the window since last reconciliation from a ledger), not the presence-derived floor, so reconciliation can repair below-floor gaps. The presence heuristic for since-floors is deleted; the coverage ledger is the sole floor source. The coverage_ledger_enabled flag defaults off during the Stage D2 ledger read-swap, so production is byte-identical to the pre-swap state; the default-on flip is a deliberate release-cut decision for external git-rev-pinning consumers. K3 keystone (coverage ledger) is complete on master: ADR-0056 (#1371), NEG-OPEN un-floor (#1372), floor soundness patches B1/B2/B3 (#1375), predicate unification locked (#1378), coverage-ledger write path (#1379), read swap default-off (#1414), eviction-ledger coherence (#1416), flag default-on (#1419), presence heuristic deleted (#1421), release nmp-v0.7.0 (#1422) and nmp-v0.7.1 (#1424). The '[LANDED M4]' claims in sync docs are corrected — the coverage ledger was dormant and is now wired.

<!-- citations: [^2e544-347] [^2e544-367] [^2e544-389] [^2e544-430] [^2e544-446] [^2e544-459] -->
## Coverage Gate and Observability

The coverage gate consults the ledger (staleness + coverage state) rather than fanout alone, and CompiledPlan includes coverage_dropped_authors for budget-exhaustion observability. <!-- [^2e544-348] -->
