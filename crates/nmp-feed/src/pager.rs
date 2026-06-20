//! Feed pull pager — ADR-0058 §8 step-6A.
//!
//! The pager is the feed-side abstraction that `load_older` rides instead of
//! the legacy `created_at` window-grow path (`window.rs` / `window_limit`,
//! both untouched here — they are removed in 6B). It turns a feed's interest
//! into a **seq-ordered** drain over the kernel pull substrate
//! ([`nmp_core::pull_page_over`] / `Kernel::pull_page`) and adapts the positive
//! log rows back into [`KernelEvent`]s the feed already ingests.
//!
//! ## Why seq, not `created_at`
//!
//! `created_at` is not arrival-monotonic: a relay can deliver an old event
//! late, landing it *behind* a `created_at` cursor, so a cursor over
//! `created_at` silently skips it (ADR-0058 §1). Pull completeness rides the
//! **ingest seq** — a late event with an old `created_at` still gets a *higher*
//! seq, so the next drain sees it. Display order is unchanged: the feed's
//! snapshot still sorts roots newest-first by `(created_at, id)`
//! (`root_indexed/engine/mod.rs`). The pager never sorts.
//!
//! ## Fail-closed interest
//!
//! A feed that cannot express its interest returns `None` from
//! [`FeedInterestShape::interest_shape`]. [`FeedPullPager::new`] (for direct
//! pager use) also returns `None`. When the pager is used via
//! [`PullFeedController`] the controller is registered unconditionally; it
//! re-reads the live shape on every `load_older` and returns `false` silently
//! when the provider yields `None` — never a broad-scan (D5).
//!
//! ## On-demand, not wake-driven
//!
//! The cursor is a feed-owned `GapAllowed` seq position (`after_seq`) called
//! synchronously on the scroll action — it is **not** a registered, long-lived
//! wake consumer. No `nmp.pull.wake` decode, no host pull accessor (ADR-0039
//! §6.1 preserved). 6B wires concrete feeds.

use nmp_core::substrate::KernelEvent;
use nmp_core::PullScope;
use nmp_planner::InterestShape;
use nmp_store::{LogOp, ScanLogResult, StoreLogEntry};

/// Default number of *visible* events one `load_older` drain targets — one page.
/// Matches the feed window page (`DEFAULT_FEED_WINDOW_LIMIT`).
pub const DEFAULT_PULL_PAGE_SIZE: usize = crate::DEFAULT_FEED_WINDOW_LIMIT;

/// Hard ceiling on rows visited per drain (D5).
pub const MAX_PULL_SCAN_BUDGET: usize = crate::MAX_FEED_WINDOW_LIMIT * 8;

/// Default cross-call scan budget: how many log rows a single drain may visit
/// before yielding, even if it has not yet filled a page. Bounds the work an
/// `InterestShape` drain does over a long unmatched run (D5: a non-matching
/// stretch can never spin unbounded). A generous multiple of the page size.
pub const DEFAULT_PULL_SCAN_BUDGET: usize = MAX_PULL_SCAN_BUDGET;

// ─── The interest seam ─────────────────────────────────────────────────────────

/// The seam each pull-backed feed implements to declare its interest.
///
/// Returning `None` is the **fail-closed** signal: the feed's interest cannot
/// be expressed as a covered [`InterestShape`], so it MUST keep rendering from
/// its existing push projection and MUST NOT broad-scan the log. The pager
/// refuses to construct in that case.
pub trait FeedInterestShape {
    /// The feed's interest as a real `InterestShape`, or `None` to fail closed.
    fn interest_shape(&self) -> Option<InterestShape>;
}

// ─── raw → KernelEvent adapter ──────────────────────────────────────────────────

/// Convert one positive (`Inserted`/`Replaced`) log row into a [`KernelEvent`].
///
/// `Deleted` rows and positive rows missing their `raw_event` payload yield
/// `None` — `InterestShape` pull only delivers positive rows (ADR-0058 §10), and
/// the feed applies deletes through its push projection, never through the pager.
/// `source_relay` becomes the event's single-entry `relay_provenance`.
#[must_use]
pub fn raw_to_kernel_event(entry: &StoreLogEntry) -> Option<KernelEvent> {
    match entry.op {
        LogOp::Inserted | LogOp::Replaced { .. } => {}
        LogOp::Deleted { .. } => return None,
    }
    let raw = entry.raw_event.as_ref()?;
    Some(KernelEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_provenance: entry.source_relay.iter().cloned().collect(),
    })
}

// ─── Drain results ──────────────────────────────────────────────────────────────

/// Why a bounded drain loop stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainStop {
    /// Collected a full page of visible events — the visible target grew.
    PageFilled,
    /// The store reported `has_more == false` — fully caught up.
    Exhausted,
    /// An explicit `PullGap` rebased the cursor; scoped continuity is not
    /// provable, the caller MUST rebase its scoped state (ADR-0058 §10).
    Gap {
        /// The seq the cursor was reset to (`first_available_seq`).
        rebased_to: u64,
    },
    /// The scan budget was spent before filling a page. Not a gap — the next
    /// drain resumes from the advanced cursor.
    ScanBudget,
}

/// Outcome of a bounded [`FeedPullPager::drain`].
#[derive(Debug)]
pub struct DrainOutcome {
    /// Positive events drained this call, in **ingest-seq** order. The caller
    /// applies them through the normal feed ingest path; display order is the
    /// snapshot's `(created_at, id)` sort, applied separately.
    pub events: Vec<KernelEvent>,
    /// Why the loop stopped.
    pub stop: DrainStop,
}

// ─── The pager ──────────────────────────────────────────────────────────────────

/// A feed-owned, on-demand seq pager over the kernel pull substrate.
///
/// Holds an optional [`InterestShape`] snapshot (for diagnostics / direct
/// pager tests) and a `GapAllowed` seq cursor (`after_seq`).
/// [`FeedPullPager::drain`] is called synchronously on the scroll action; it
/// advances `after_seq` only after a page is processed.
///
/// When constructed via [`FeedPullPager::at_start`] no shape is stored: the
/// pager acts as a pure seq cursor. [`PullFeedController`] uses this path so
/// the controller can be registered **before** sign-in — `load_older` reads
/// the live shape from the provider on every call and fails closed if `None`.
#[derive(Clone, Debug)]
pub struct FeedPullPager {
    shape: Option<InterestShape>,
    after_seq: u64,
    page_size: usize,
    scan_budget: usize,
}

impl FeedPullPager {
    /// Construct a pager from a feed's interest seam, or `None` if the feed
    /// fails closed (its interest is not a real `InterestShape`). Use this
    /// for direct pager tests that need to inspect the stored shape.
    #[must_use]
    pub fn new(provider: &dyn FeedInterestShape) -> Option<Self> {
        let shape = provider.interest_shape()?;
        Some(Self {
            shape: Some(shape),
            after_seq: 0,
            page_size: DEFAULT_PULL_PAGE_SIZE,
            scan_budget: DEFAULT_PULL_SCAN_BUDGET,
        })
    }

    /// Construct a cursor-only pager starting at `seq` with no stored shape.
    ///
    /// Used by [`PullFeedController`], which re-reads the live shape on every
    /// `load_older` call via its provider. This allows the controller to be
    /// registered unconditionally — before an account is signed in — and begin
    /// pulling as soon as the provider yields a real shape.
    #[must_use]
    pub fn at_start() -> Self {
        Self {
            shape: None,
            after_seq: 0,
            page_size: DEFAULT_PULL_PAGE_SIZE,
            scan_budget: DEFAULT_PULL_SCAN_BUDGET,
        }
    }

    /// Override page size / scan budget (clamped to the hard ceiling).
    #[must_use]
    pub fn with_budgets(mut self, page_size: usize, scan_budget: usize) -> Self {
        self.page_size = page_size.max(1);
        self.scan_budget = scan_budget.clamp(1, MAX_PULL_SCAN_BUDGET);
        self
    }

    /// The stored interest shape, if any. `None` for cursor-only pagers
    /// created via [`FeedPullPager::at_start`].
    #[must_use]
    pub fn shape(&self) -> Option<&InterestShape> {
        self.shape.as_ref()
    }

    /// The pull scope to hand the kernel, if a shape is stored.
    #[must_use]
    pub fn pull_scope(&self) -> Option<PullScope> {
        self.shape
            .as_ref()
            .map(|s| PullScope::InterestShape(s.clone()))
    }

    /// The cursor's current seq position (last fully-consumed seq).
    #[must_use]
    pub fn after_seq(&self) -> u64 {
        self.after_seq
    }

    /// Bounded on-demand drain — the loop `load_older` rides.
    ///
    /// Repeatedly calls `pull_fn(after_seq)` (in production:
    /// `kernel.pull_page(self.pull_scope(), after_seq, limits)`), converting
    /// each positive row to a [`KernelEvent`]. Terminates when **any** of:
    /// the visible target grows by one page (`PageFilled`), the store reports
    /// `has_more == false` (`Exhausted`), a `PullGap` rebases the cursor
    /// (`Gap`), or the scan budget is spent (`ScanBudget`).
    ///
    /// `after_seq` advances **only after** a page is processed. On a gap the
    /// cursor is reset to `first_available_seq` and the loop stops — the caller
    /// must rebase its scoped state, never silently claim continuity.
    pub fn drain(&mut self, mut pull_fn: impl FnMut(u64) -> ScanLogResult) -> DrainOutcome {
        let mut events: Vec<KernelEvent> = Vec::new();
        let mut scanned: usize = 0;

        loop {
            let page = match pull_fn(self.after_seq) {
                ScanLogResult::Gap(gap) => {
                    // Explicit rebase — scoped continuity is not provable.
                    self.after_seq = gap.first_available_seq;
                    return DrainOutcome {
                        events,
                        stop: DrainStop::Gap {
                            rebased_to: gap.first_available_seq,
                        },
                    };
                }
                ScanLogResult::Page(page) => page,
            };

            let rows = page.entries.len();
            let old_after_seq = self.after_seq;
            let advanced = page.next_after_seq > self.after_seq;
            for entry in &page.entries {
                if let Some(ev) = raw_to_kernel_event(entry) {
                    events.push(ev);
                }
            }
            // The cursor advances to the page's next_after_seq (a valid kernel
            // page never moves it backward; assert that loudly in debug). The
            // `.max` is a release-mode floor so a malformed page can never
            // rewind the cursor.
            debug_assert!(
                page.next_after_seq >= self.after_seq,
                "pull page next_after_seq moved the cursor backward"
            );
            // Advance the cursor ONLY after the page is processed.
            self.after_seq = page.next_after_seq.max(self.after_seq);
            // Count VISITED log rows toward the scan budget — the seq delta, NOT
            // the number of returned (matching) entries. An InterestShape page
            // can advance over a long run of non-matching / Deleted rows and
            // return `entries == []` with `has_more == true`; counting only
            // returned entries would let one drain walk the whole log and defeat
            // the per-drain budget (D5). The seq space is dense (one seq per log
            // row), so `next_after_seq - old_after_seq` is the rows visited.
            let visited = self.after_seq.saturating_sub(old_after_seq);
            scanned = scanned.saturating_add(visited as usize);

            // Caught up: nothing more to drain (covers the empty-but-advancing
            // final page — it terminates, it does not poll).
            if !page.has_more {
                return DrainOutcome {
                    events,
                    stop: DrainStop::Exhausted,
                };
            }
            // Visible target grew by a page.
            if events.len() >= self.page_size {
                return DrainOutcome {
                    events,
                    stop: DrainStop::PageFilled,
                };
            }
            // Defensive: a page that claims `has_more` yet neither advanced the
            // cursor nor produced rows would otherwise spin. Stop — never poll.
            if !advanced && rows == 0 {
                return DrainOutcome {
                    events,
                    stop: DrainStop::ScanBudget,
                };
            }
            // Bounded work per drain (D5): a long unmatched run yields.
            if scanned >= self.scan_budget {
                return DrainOutcome {
                    events,
                    stop: DrainStop::ScanBudget,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests;
