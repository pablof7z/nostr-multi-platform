//! K3 Stage D1 (ADR-0056 §3) — coverage-ledger WRITE path on the kernel.
//!
//! Split out of `kernel/mod.rs` (LOC cap) as a cohesive owner: the flag, the
//! two completion entry points (EOSE for a plain REQ, NEG-DONE for a NIP-77
//! negentropy reconciliation), and the canonical-key extraction all live here.
//! The store-level row type and read/write primitives live in `nmp-store`
//! (`CoverageRow`, `EventStore::record_coverage` / `get_coverage`).
//!
//! D1 is WRITE-only and OFF by default: with `coverage_ledger_enabled == false`
//! nothing is recorded and nothing reads the ledger; the since-floor stays
//! presence-derived until the Stage D2 read swap.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nmp_store::EventStore;

use super::cache_serve;
use super::Kernel;
use crate::planner::{canonical_filter_hash, InterestShape};
use crate::subs::WatermarkFn;

/// Build the kernel's installed since-floor resolver (`WatermarkFn`) — the
/// cohesive owner of BOTH the presence-floor scan (T129 / ADR-0045 §6) and the
/// K3 Stage D2 coverage-ledger read swap. Extracted from `Kernel::new` so the
/// constructor stays under the file-size cap and the floor logic lives next to
/// the ledger write path it reads.
///
/// Captured state (all by `Arc` clone, since the closure outlives `new` on the
/// lifecycle): `store` (presence scans + ledger reads off the SAME handle —
/// "one table read two ways"), `flag` (the shared `Arc<AtomicBool>` the kernel
/// toggles via `set_coverage_ledger_enabled`, read LIVE per recompile), and
/// `truncated_query_keys` (the K3 Stage B3 cursor-less truncation read view).
///
/// The resolver is invoked per non-ephemeral sub-shape with the relay the REQ
/// targets; it returns the floor BASE (the `+1` is applied by the
/// floor-application sites `apply_watermark_rewrite` / `handle_reconnect`).
pub(super) fn build_watermark_fn(
    store: Arc<dyn EventStore>,
    flag: Arc<AtomicBool>,
    truncated_query_keys: Arc<Mutex<HashSet<u64>>>,
) -> WatermarkFn {
    Arc::new(move |shape: &InterestShape, relay_url: &str| {
        // The presence floor (used on the flag-OFF path, and never on flag-ON
        // per the read-swap decision table). ADR-0045 §6 / #1119: derived from
        // the SAME `shape_to_store_queries` mapping cache-serve uses ("one table
        // read two ways"). `watermark_from_queries` folds the per-query newest
        // timestamps with the established policy (min across AuthorKind with
        // abort-on-empty author, min across KindDtag coords with abort-on-empty
        // coord (K3 Stage B1), single value for Etag/Ptag, never-floor for the
        // zero-author KindTime global feed). The scan normalizes each query to
        // its watermark form (since/until = None) and reads the newest stored
        // match via a `limit = 1` early-stopping `query_visit` (D8: one u64 per
        // query, no per-emit allocation). It is a closure so these scans run
        // ONLY when actually needed (flag off), never under the ledger.
        let presence_fn = || {
            cache_serve::watermark_from_queries(
                shape,
                |query| {
                    let mut q = query.clone();
                    if let Some(since) = cache_serve::query_since_mut(&mut q) {
                        *since = None;
                    }
                    if let Some(until) = cache_serve::query_until_mut(&mut q) {
                        *until = None;
                    }
                    let mut ts: Option<u64> = None;
                    let _ = store.query_visit(&q, 1, &mut |ev| {
                        ts = Some(ev.raw.created_at);
                        std::ops::ControlFlow::Break(())
                    });
                    ts
                },
                // K3 Stage B3 / #1380: refuse the floor for a cursor-less shape
                // whose serve was budget-truncated this session. `key` is the
                // query-content key; the captured read view holds it iff AT
                // LEAST ONE active interest mapping to that query is truncated,
                // so any contributing interest's truncation refuses the shared
                // merged-REQ floor (the conservative, correct merge).
                |key| {
                    truncated_query_keys
                        .lock()
                        .map(|set| set.contains(&key))
                        .unwrap_or(false)
                },
            )
        };
        // K3 Stage D2 read swap — the SHARED decision table (the same logic the
        // `&self` wrapper `Kernel::coverage_floor_for`, which the unit tests
        // drive, calls). One mapping, read two ways; no second hand-synced copy.
        coverage_floor_with_fallback(&flag, &store, shape, relay_url, presence_fn)
    })
}

/// K3 Stage D2 (ADR-0056 §3.D2) — the since-floor read-swap decision table, as
/// a free function so the SINGLE definition is shared by both floor-read sites:
///
/// 1. the installed [`crate::subs::WatermarkFn`] closure (`kernel/mod.rs`),
///    which cannot hold `&Kernel` (it is owned by the lifecycle, a sibling of
///    the kernel), so it captures the flag + store `Arc`s and calls this; and
/// 2. [`Kernel::coverage_floor_for`] (below), the `&self` wrapper the D2 unit
///    tests drive directly.
///
/// One decision table, read two ways — the same Stage-C discipline that
/// collapsed the three presence-floor copies. There is no second hand-synced
/// copy of the swap logic to drift.
///
/// `presence_fn` computes the legacy presence-derived floor (newest stored
/// match for the shape). Under D2 it is consulted ONLY on the flag-OFF path —
/// it is a closure so its (potentially several) `query_visit` store scans never
/// run when the ledger governs the floor.
///
/// Decision table:
///
/// - **Flag OFF** → `presence_fn()` (exactly today's behaviour; the relay is
///   ignored, the ledger is never consulted — D2 is a dormant no-op).
/// - **Flag ON, ledger HAS a row** for `(canonical_filter_hash(shape), relay)`
///   → `Some(covered_through)`. Coverage is the sound floor source: the relay
///   has completed a sync through `covered_through`, so a REQ may honestly
///   floor `since` to `covered_through + 1` (the `+1` is applied by the
///   floor-application sites `apply_watermark_rewrite` / `handle_reconnect`).
/// - **Flag ON, ledger has NO row** (un-synced `(filter_hash, relay)`) → `None`
///   — **refuse to floor** (ADR-0056 §3.D2 item 2: "no completed-coverage row ⇒
///   refuse to floor"). The REQ runs un-floored over the full `[0, ∞)` window.
///
/// # Why no-row REFUSES the floor rather than falling back to presence
///
/// The whole premise of the ledger (ADR-0056 §1) is *"presence is not
/// coverage."* The H1 headline is precisely the case where presence is unsound:
/// a single stray event by author A (a thread reply stored under an Etag shape)
/// makes the presence floor for A's follow-feed shape `Some(stray_ts)`, which
/// suppresses A's history below the stray — *even though that shape has never
/// completed a sync against this relay*. If no-row fell back to presence, the
/// follow-feed would re-inherit that poisoned floor and the merge-gate scenario
/// (§4: "follow a user AFTER a thread reply … the author's FULL history
/// backfills") would FAIL. Refusing the floor when the ledger has no completed-
/// coverage row is therefore the load-bearing fix: an un-synced `(filter_hash,
/// relay)` fetches the full window, the relay's EOSE / NEG-DONE then records
/// honest coverage, and *subsequent* syncs floor at the recorded
/// `covered_through`.
///
/// This is the "migration safety / never a worse one" property ADR-0056 §3.D2
/// item 1 names, read soundly: a full backfill is never WORSE than a presence
/// floor — it can only fetch MORE, never suppress. The cost is a one-time full
/// re-fetch per shape the first time the flag is enabled, after which the
/// ledger floors normally. The presence heuristic survives ONLY behind the
/// default-off flag, to be deleted entirely in Stage E.
pub(crate) fn coverage_floor_with_fallback(
    flag: &AtomicBool,
    store: &Arc<dyn EventStore>,
    shape: &InterestShape,
    relay_url: &str,
    presence_fn: impl FnOnce() -> Option<u64>,
) -> Option<u64> {
    if !flag.load(Ordering::Relaxed) {
        return presence_fn();
    }
    let filter_hash = canonical_filter_hash(shape);
    // Flag ON: coverage — not presence — is the floor authority.
    //   row present ⇒ floor at the honest completed-coverage bound;
    //   no row      ⇒ refuse to floor (full `[0, ∞)` window, the H1 fix).
    store.get_coverage(&filter_hash, relay_url)
}

impl Kernel {
    /// K3 Stage D2 (ADR-0056 §3.D2) — resolve the since-floor base for
    /// `(shape, relay)`, reading the coverage ledger with a presence fallback.
    ///
    /// Thin `&self` wrapper over [`coverage_floor_with_fallback`] (the shared
    /// decision table) for callers that hold a `&Kernel` — chiefly the D2 unit
    /// tests, which drive the table directly without standing up the full
    /// recompile/WireFrame path. The installed `WatermarkFn` closure calls the
    /// same free function with the same captured flag + store handle, so the
    /// two floor-read sites are guaranteed identical.
    ///
    /// Test-only: production never calls this (the floor is read through the
    /// installed `WatermarkFn` closure, which calls the free function directly).
    /// It exists purely so the D2 unit tests can drive the shared decision table
    /// with a `&Kernel` in hand.
    #[cfg(test)]
    pub(crate) fn coverage_floor_for(
        &self,
        shape: &InterestShape,
        relay_url: &str,
        presence_fn: impl FnOnce() -> Option<u64>,
    ) -> Option<u64> {
        coverage_floor_with_fallback(
            &self.coverage_ledger_enabled,
            &self.store,
            shape,
            relay_url,
            presence_fn,
        )
    }

    /// Enable/disable the coverage-ledger WRITE path. Default `false`.
    ///
    /// With the flag off the kernel records no coverage at EOSE / NEG-DONE (a
    /// pure no-op); with it on the ledger fills, but READ behaviour is unchanged
    /// in D1 (the since-floor is swapped to read the ledger only in Stage D2).
    pub fn set_coverage_ledger_enabled(&mut self, enabled: bool) {
        // Shared `Arc<AtomicBool>` with the installed `WatermarkFn` closure
        // (K3 Stage D2) — a write here is observed by the next recompile's
        // floor read. `Relaxed` is sufficient: the flag gates a heuristic, not
        // a memory-safety invariant, and there is no companion state that must
        // be published with it.
        self.coverage_ledger_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the coverage-ledger write path is enabled.
    #[must_use]
    pub fn coverage_ledger_enabled(&self) -> bool {
        self.coverage_ledger_enabled.load(Ordering::Relaxed)
    }

    /// Record completed coverage at NEG-DONE.
    ///
    /// Called from the NIP-77 runtime (`nmp-nip77::runtime`) when a negentropy
    /// reconciliation reaches its terminal `Done` outcome for `(sub_id, relay)`.
    /// Per ADR-0056 Stage A the NEG reconciliation runs **un-floored** over the
    /// full `[0, ∞)` window, so a completed reconciliation honestly covers
    /// `[0, now]` — the downward-closed ledger is advanced to `now`
    /// unconditionally (no floor to guard against, unlike the plain-REQ EOSE
    /// path). Gated on the off-by-default flag inside `record_coverage_complete`.
    ///
    /// `now_secs` is threaded in by the caller (the NIP-77 runtime already reads
    /// `kernel.now_secs()` for its liveness deadline) so this method does not
    /// re-read the clock — a single clock read per terminal event.
    pub fn record_neg_done_coverage(&self, sub_id: &str, relay_url: &str, now_secs: u64) {
        self.record_coverage_complete(sub_id, relay_url, now_secs);
    }

    /// Record completed coverage at EOSE for a plain REQ.
    ///
    /// The relay has sent everything it has in the REQ window, so `[since_floor,
    /// now]` is covered. We advance the downward-closed ledger ONLY for an
    /// un-floored REQ (`since_floor` absent or `0`), which honestly proves
    /// `[0, now]`; a `since`-floored REQ proves only `[floor, now]`, so it
    /// records NO coverage rather than over-claim `[0, floor)` (the over-claim
    /// ADR-0056 §1 says makes presence unsound). Gated on the off-by-default
    /// flag inside `record_coverage_complete`.
    pub(crate) fn record_eose_coverage(
        &self,
        sub_id: &str,
        relay_url: &str,
        since_floor: Option<u64>,
        now_secs: u64,
    ) {
        let covered_through = match since_floor {
            None | Some(0) => now_secs,
            Some(_floor) => 0,
        };
        self.record_coverage_complete(sub_id, relay_url, covered_through);
    }

    /// Record completed coverage for a wire sub, keyed by the canonical filter
    /// hash extracted from `sub_id`.
    ///
    /// `sub_id` is the planner-assigned wire id (`sub-<canonical_filter_hash>`);
    /// the hash after the `sub-` prefix is the ledger key half so the Stage D2
    /// read swap finds the row by the SAME key `recompile` builds.
    /// `covered_through` is the upper bound of the **downward-closed** window
    /// the completion proved `[0, covered_through]` — `0` is a no-op (no row).
    ///
    /// Gated on the off-by-default flag: with the flag off this is a no-op and
    /// no row is ever written. D6 graceful degrade: the store seam swallows
    /// write errors, so a failed ledger write never blocks the EOSE/NEG path.
    pub(crate) fn record_coverage_complete(
        &self,
        sub_id: &str,
        relay_url: &str,
        covered_through: u64,
    ) {
        if !self.coverage_ledger_enabled.load(Ordering::Relaxed) {
            return;
        }
        // Only planner `sub-<hash>` ids carry a canonical filter hash; the legacy
        // `seed-timeline` / `diag-firehose-` / oneshot ids do not map to a
        // recompile floor key, so there is nothing the ledger could be read by in
        // D2. Skip them rather than invent a non-canonical key.
        let Some(filter_hash) = sub_id.strip_prefix("sub-") else {
            return;
        };
        self.store
            .record_coverage(filter_hash, relay_url, covered_through);
    }
}
