//! Kernel pull service — ADR-0058 §10, step 2.
//!
//! Exposes `Kernel::pull_page` over two scopes:
//!
//! - `PullScope::GlobalLog` — returns the raw store log exactly as
//!   `scan_log_since_seq` returns it (includes Inserted, Replaced, Deleted).
//! - `PullScope::InterestShape` — bounded post-filter over the GlobalLog scan;
//!   includes only Inserted/Replaced rows that match the shape predicate;
//!   Deleted rows advance the seq cursor but are never yielded (ADR §10).
//!
//! ## What this step does NOT include (step-3+/step-4)
//!
//! No FFI, no wake signal (`nmp.pull.wake`), no cursor registry, no
//! `PullCursorId`, no durable cursor storage, no GC log-floor pins, no
//! polling or timers.

mod predicate;

use std::num::NonZeroUsize;

use crate::kernel::Kernel;
use crate::planner::InterestShape;
use crate::store::{EventStore, PullPage, ScanLogResult, StoreError};
use crate::store::{LogOp, StoreLogEntry};
use predicate::raw_matches_shape;

use super::cache_serve::queries::shape_to_store_queries;

// ─── Public types ─────────────────────────────────────────────────────────────

/// What portion of the ingest log to scan.
#[derive(Clone, Debug)]
pub enum PullScope {
    /// The entire ingest log — delivers Inserted, Replaced, **and** Deleted rows.
    GlobalLog,
    /// Post-filtered view: only Inserted/Replaced rows that match this shape.
    /// Deleted rows advance the cursor but are never yielded (ADR §10).
    InterestShape(InterestShape),
}

/// Bounds controlling a single `pull_page` call.
#[derive(Clone, Copy, Debug)]
pub struct PullLimits {
    /// Maximum number of matching entries to return.
    pub max_entries: NonZeroUsize,
    /// Maximum number of log rows to visit (scan budget, for InterestShape).
    /// For GlobalLog this is also the page size delivered to the store.
    pub max_scan_entries: NonZeroUsize,
}

/// Errors from `Kernel::pull_page`.
#[derive(Debug)]
pub enum PullError {
    /// The `InterestShape` could not be compiled to any `StoreQuery` —
    /// wildcard kinds, event-id-only, multi-tag intersections, etc.
    UnsupportedInterestShape,
    /// Limits were logically invalid (currently unused; reserved for future
    /// validation that `max_entries ≤ max_scan_entries`).
    InvalidLimits,
    /// The underlying store returned an error.
    Store(StoreError),
}

impl From<StoreError> for PullError {
    fn from(e: StoreError) -> Self {
        PullError::Store(e)
    }
}

/// Return type of `Kernel::pull_page` — a normal `ScanLogResult` (Page or Gap).
pub type KernelPullResult = ScanLogResult;

// ─── Free function (testable core) ───────────────────────────────────────────

/// Scan the ingest log from `after_seq` (exclusive), returning up to
/// `limits.max_entries` matching rows.
///
/// This is the behavior-preserving extracted body of `Kernel::pull_page`.
/// `Kernel::pull_page` delegates here; tests inject a fake store directly.
///
/// ## GlobalLog
///
/// Direct pass-through to `store.scan_log_since_seq(after_seq,
/// limits.max_entries)`. Includes Inserted, Replaced, and Deleted rows.
/// Propagates `PullGap` unchanged.
///
/// ## InterestShape
///
/// 1. Compiles the shape to `StoreQuery`s; rejects with
///    `PullError::UnsupportedInterestShape` if the shape produces no
///    queries.
/// 2. Calls `scan_log_since_seq(after_seq, limits.max_scan_entries)`.
/// 3. Visits rows ascending by seq, counting towards two budgets:
///    - **scan budget** (`max_scan_entries`): every row visited counts,
///      even skipped Deleted rows.
///    - **entry limit** (`max_entries`): only matching
///      Inserted/Replaced rows count.
/// 4. `next_after_seq` is set to the **last visited row's seq** when the
///    loop stops early at `max_entries`; the store's `next_after_seq` is
///    used as-is when the loop ran to exhaustion or when a `PullGap` is
///    returned.
pub(crate) fn pull_page_over(
    store: &dyn EventStore,
    scope: PullScope,
    after_seq: u64,
    limits: PullLimits,
) -> Result<KernelPullResult, PullError> {
    match scope {
        PullScope::GlobalLog => {
            let result = store.scan_log_since_seq(after_seq, limits.max_entries.get())?;
            Ok(result)
        }

        PullScope::InterestShape(shape) => {
            // Step 1: compile the shape — reject unsupported shapes.
            let queries = shape_to_store_queries(&shape);
            if queries.is_empty() {
                return Err(PullError::UnsupportedInterestShape);
            }

            // Step 2: scan the global log up to the scan budget.
            let scan_result =
                store.scan_log_since_seq(after_seq, limits.max_scan_entries.get())?;

            // Propagate a real gap unchanged (ADR §10: gap contract).
            let page = match scan_result {
                ScanLogResult::Gap(gap) => return Ok(ScanLogResult::Gap(gap)),
                ScanLogResult::Page(p) => p,
            };

            // Steps 3–4: filter the page.
            filter_page(page, &queries, &shape, limits.max_entries.get())
        }
    }
}

// ─── Kernel impl ─────────────────────────────────────────────────────────────

impl Kernel {
    /// Scan the ingest log from `after_seq` (exclusive), returning up to
    /// `limits.max_entries` matching rows.
    ///
    /// Delegates to [`pull_page_over`] with this kernel's store. See that
    /// function for the full semantics.
    pub fn pull_page(
        &self,
        scope: PullScope,
        after_seq: u64,
        limits: PullLimits,
    ) -> Result<KernelPullResult, PullError> {
        pull_page_over(&*self.store, scope, after_seq, limits)
    }
}

// ─── InterestShape post-filter ────────────────────────────────────────────────

/// Apply the predicate and budget logic over a raw `PullPage`, producing a
/// filtered `KernelPullResult`.
///
/// Semantics (ADR §10):
/// - Deleted rows are skipped **but still advance the scan cursor**.
/// - Inserted/Replaced rows that match the predicate are collected.
/// - Loop stops when `max_entries` matches are collected **or** all rows are
///   visited.
/// - When stopped early by `max_entries`: `next_after_seq` is set to the last
///   visited row's seq (not the store page's `next_after_seq`).
/// - An empty-but-advancing page is correct and is **not** a gap.
fn filter_page(
    page: PullPage,
    queries: &[crate::store::StoreQuery],
    shape: &InterestShape,
    max_entries: usize,
) -> Result<KernelPullResult, PullError> {
    let store_next_after_seq = page.next_after_seq;
    let latest_seq = page.latest_seq;

    let mut collected: Vec<StoreLogEntry> = Vec::new();
    let mut last_visited_seq: Option<u64> = None;
    let mut stopped_early = false;

    for entry in page.entries {
        last_visited_seq = Some(entry.seq);

        match &entry.op {
            // Deleted rows: advance cursor but do not yield (ADR §10).
            LogOp::Deleted { .. } => {
                // do nothing — seq already recorded in last_visited_seq
            }
            // Positive rows: check predicate.
            LogOp::Inserted | LogOp::Replaced { .. } => {
                if let Some(raw) = &entry.raw_event {
                    if raw_matches_shape(raw, queries, shape) {
                        collected.push(entry);
                        if collected.len() >= max_entries {
                            stopped_early = true;
                            break;
                        }
                    }
                }
                // If raw_event is None for an Inserted/Replaced (shouldn't
                // happen per the log contract, but be safe), skip it.
            }
        }
    }

    // Determine next_after_seq:
    // - If stopped early at max_entries: use last_visited_seq (the loop
    //   broke after recording last_visited_seq but before advancing further).
    // - Otherwise: use the store page's next_after_seq (which equals the
    //   seq of the last row scanned, or the original after_seq if the page
    //   was empty).
    let next_after_seq = if stopped_early {
        last_visited_seq.unwrap_or(store_next_after_seq)
    } else {
        store_next_after_seq
    };

    let has_more = next_after_seq < latest_seq;

    Ok(ScanLogResult::Page(PullPage {
        entries: collected,
        next_after_seq,
        latest_seq,
        has_more,
    }))
}
