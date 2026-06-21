//! `PullFeedController` — ADR-0058 §8 step-6B.
//!
//! The single, on-demand paging path a feed rides when a host reports
//! `nmp_app_load_older_feed`. It replaces every per-feed `created_at`
//! window-grow `load_older` (deleted in 6B): there is now exactly ONE paging
//! mechanism, the seq-ordered pull drain.
//!
//! ## What `load_older` does
//!
//! 1. Re-reads the feed's live [`InterestShape`] from its provider. `None`
//!    fails closed — no broad-scan, the feed keeps rendering its push
//!    projection (D5).
//! 2. Runs a bounded [`FeedPullPager`] drain over the kernel pull substrate
//!    (`pull_fn`, injected by the composition root from the in-process event
//!    store — NOT a new host pull accessor; ADR-0039 §6.1 preserved).
//! 3. Applies every drained positive row through the feed's **own** ingest path
//!    (`apply`, the same `KernelEventObserver::on_kernel_event` the push fan-out
//!    uses), so dedup + snapshot projection are identical to live ingest.
//! 4. Grows the render viewport one page (`advance`) **after** the page is
//!    ingested, so the newly-arrived (possibly older-`created_at`) roots become
//!    visible in the `(created_at, id)`-sorted snapshot.
//!
//! ## On-demand, not wake-driven
//!
//! `load_older` is called **synchronously** on the scroll action. It does not
//! register, decode, or consume `nmp.pull.wake`; the pager owns a private
//! `GapAllowed` seq cursor. Completeness rides ingest seq — a late event with an
//! old `created_at` lands at a *higher* seq, so the next drain sees it even
//! though a `created_at` cursor would have skipped it (ADR-0058 §1).

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use nmp_core::planner::InterestShape;
use nmp_core::store::{EventStore, PullPage, ScanLogResult};
use nmp_core::{pull_page_over, PullLimits, PullScope};

use nmp_core::substrate::KernelEvent;

use crate::pager::{FeedInterestShape, FeedPullPager, DEFAULT_PULL_PAGE_SIZE};
use crate::FeedController;

/// The current `Arc<dyn EventStore>`, or `None` when no store is available yet.
///
/// Native reads this through the kernel's republished event-store slot (so a
/// `Reset` that mints a fresh store is observed without re-wiring); the wasm/web
/// path returns a stable `Arc` clone. `None` ⇒ the built [`PullFn`] yields an
/// empty, exhausted page (fail closed — no broad-scan).
pub type EventStoreProvider = Arc<dyn Fn() -> Option<Arc<dyn EventStore>> + Send + Sync>;

/// Build a [`PullFn`] over an [`EventStoreProvider`].
///
/// This is the ONE in-process pull seam both the native composition root
/// (`NmpApp::feed_pull_fn`) and the wasm/web composition root
/// (`nmp_app_chirp_web`) hand to their [`PullFeedController`], so the
/// page-size / scan-budget policy and the fail-closed terminator live in
/// exactly one place — never a platform-specific fork (ADR-0039 §6.1: a plain
/// Rust closure, never a new host pull accessor).
///
/// Limits: one match entry per visible row (`DEFAULT_PULL_PAGE_SIZE`) and a
/// generous per-call scan window (`8 ×`); the pager's own cross-call scan
/// budget bounds total work (D5). On an unavailable store (provider yields
/// `None`) or an unsupported / erroring scope, the closure returns an **empty,
/// exhausted page** so the pager drain terminates and the feed fails closed
/// (no broad-scan, no poll).
#[must_use]
pub fn pull_fn_from_store_provider(store_provider: EventStoreProvider) -> PullFn {
    let max_entries = NonZeroUsize::new(DEFAULT_PULL_PAGE_SIZE).unwrap_or(NonZeroUsize::MIN);
    let max_scan =
        NonZeroUsize::new(DEFAULT_PULL_PAGE_SIZE.saturating_mul(8)).unwrap_or(NonZeroUsize::MIN);
    let limits = PullLimits {
        max_entries,
        max_scan_entries: max_scan,
    };
    Arc::new(move |scope: PullScope, after_seq: u64| {
        // Fail-closed terminator: an empty page at the requested cursor with
        // `has_more == false` ⇒ the drain stops as `Exhausted`, applies and
        // grows nothing, and `load_older` returns false (projection-only).
        let exhausted = || {
            ScanLogResult::Page(PullPage {
                entries: Vec::new(),
                next_after_seq: after_seq,
                latest_seq: after_seq,
                has_more: false,
            })
        };
        let Some(store) = store_provider() else {
            return exhausted();
        };
        match pull_page_over(store.as_ref(), scope, after_seq, limits) {
            Ok(result) => result,
            // Unsupported shape / store error ⇒ fail closed, never broad-scan.
            Err(_) => exhausted(),
        }
    })
}

/// The in-process pull seam: `(scope, after_seq) -> page`. The composition root
/// builds this over the kernel event store (`nmp_core::pull_page_over`); it is a
/// plain Rust closure, never a new C-ABI symbol (ADR-0039 §6.1). On an
/// unsupported shape or unavailable store it MUST return an empty, exhausted
/// page so the drain terminates and the feed fails closed (no broad-scan).
pub type PullFn = Arc<dyn Fn(PullScope, u64) -> ScanLogResult + Send + Sync>;

/// Apply one drained positive row through the feed's own ingest path (the same
/// path the push fan-out uses — dedup + snapshot projection unchanged).
pub type FeedApply = Arc<dyn Fn(&KernelEvent) + Send + Sync>;

/// Grow the feed's render viewport by one page, called after a drained page is
/// ingested. Return value (if any) is ignored by the controller.
pub type FeedAdvance = Arc<dyn Fn() + Send + Sync>;

/// A [`FeedInterestShape`] backed by a closure, so a feed's live interest (e.g.
/// the active account's follow set + host-declared kinds) is re-evaluated on
/// every `load_older` rather than frozen at registration. Returning `None`
/// fails closed.
pub struct ClosureInterestShape<F>(F);

impl<F> ClosureInterestShape<F>
where
    F: Fn() -> Option<InterestShape> + Send + Sync,
{
    /// Wrap a closure as a fail-closed interest provider.
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> FeedInterestShape for ClosureInterestShape<F>
where
    F: Fn() -> Option<InterestShape> + Send + Sync,
{
    fn interest_shape(&self) -> Option<InterestShape> {
        (self.0)()
    }
}

/// The pull-backed [`FeedController`]. Owns the feed's pager (seq cursor) and the
/// three injected closures; see the module docs for the `load_older` contract.
pub struct PullFeedController {
    pager: Mutex<FeedPullPager>,
    provider: Arc<dyn FeedInterestShape + Send + Sync>,
    pull: PullFn,
    apply: FeedApply,
    advance: FeedAdvance,
}

impl PullFeedController {
    /// Construct a pull-backed controller. Always succeeds — registration is
    /// unconditional so the controller is present before sign-in. The provider
    /// re-reads the live shape on every [`load_older`](FeedController::load_older)
    /// call; `None` from the provider fails closed (no pull, no broad-scan).
    ///
    /// This means the caller MUST NOT guard on `None` to decide whether to
    /// register: always register, let the controller fail closed via its provider.
    #[must_use]
    pub fn new(
        provider: Arc<dyn FeedInterestShape + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        advance: FeedAdvance,
    ) -> Arc<Self> {
        // Cursor-only pager — no initial shape check. The live shape is read on
        // every load_older call, so a controller registered before sign-in
        // becomes active as soon as the provider yields a real shape.
        let pager = FeedPullPager::at_start();
        Arc::new(Self {
            pager: Mutex::new(pager),
            provider,
            pull,
            apply,
            advance,
        })
    }

    /// The pager's current seq cursor (diagnostics / tests).
    #[must_use]
    pub fn after_seq(&self) -> u64 {
        self.pager.lock().map(|p| p.after_seq()).unwrap_or(0)
    }
}

impl FeedController for PullFeedController {
    fn load_older(&self) -> bool {
        // Live, fail-closed interest. If the feed can no longer express a covered
        // shape (e.g. logout cleared the follow set), do nothing — never scan.
        let Some(shape) = self.provider.interest_shape() else {
            return false;
        };
        let scope = PullScope::InterestShape(shape);

        // Bounded seq-ordered drain. The pager owns the cursor and the budget;
        // it terminates on PageFilled / Exhausted / Gap / ScanBudget — it never
        // polls (an empty-but-advancing final page returns Exhausted at once).
        let outcome = {
            let Ok(mut pager) = self.pager.lock() else {
                return false;
            };
            let pull = &self.pull;
            pager.drain(|after_seq| pull(scope.clone(), after_seq))
        };

        // Apply the page through the feed's OWN ingest path FIRST (dedup +
        // projection), THEN grow the viewport so the just-ingested roots count
        // toward what becomes visible. A `Gap` already rebased the pager cursor
        // explicitly (no silent continuity claim); any rows drained before it
        // are still applied here.
        for event in &outcome.events {
            (self.apply)(event);
        }
        let progressed = !outcome.events.is_empty();
        if progressed {
            (self.advance)();
        }
        progressed
    }
}

#[cfg(test)]
mod tests;
