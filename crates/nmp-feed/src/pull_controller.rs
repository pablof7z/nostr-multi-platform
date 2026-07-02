//! `PullFeedController` — ADR-0072 §8 step-6B.
//!
//! The single, on-demand paging path a feed rides when a host reports
//! `load_older_feed` through the native session API. It replaces every
//! per-feed `created_at` window-grow `load_older` (deleted in 6B): there is now exactly ONE paging
//! mechanism, the seq-ordered pull drain.
//!
//! ## What `load_older` does
//!
//! 1. Re-reads the feed's live [`InterestShape`] from its provider. `None`
//!    fails closed — no broad-scan, the feed keeps rendering its push
//!    projection (D5).
//! 2. Runs a bounded [`FeedPullPager`] drain over the kernel pull substrate
//!    (`pull_fn`, injected by the composition root from the in-process event
//!    store — NOT a new host pull accessor; ADR-0070 §6.1 preserved).
//! 3. Applies every drained positive row through the feed's **own** ingest path
//!    (`apply`, the same `ObservedProjectionSink::on_kernel_event` the push fan-out
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
//! though a `created_at` cursor would have skipped it (ADR-0072 §1).

use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::{PullLimits, PullScope};
use nmp_planner::InterestShape;
use nmp_store::ScanLogResult;

use crate::pager::{FeedInterestShape, FeedPullPager};
use crate::{FeedController, FeedLoadStatus, FeedLoadStopReason, FeedWindowPolicy};

/// The in-process pull seam: `(scope, after_seq) -> page`. The composition root
/// builds this over the kernel event store (`nmp_core::pull_page_over`); it is a
/// plain Rust closure, never a new C-ABI symbol (ADR-0070 §6.1). On an
/// unsupported shape or unavailable store it MUST return an empty, exhausted
/// page so the drain terminates and the feed fails closed (no broad-scan).
pub type PullFn = Arc<dyn Fn(PullScope, u64, PullLimits) -> ScanLogResult + Send + Sync>;

/// Apply one drained positive row through the feed's own ingest path (the same
/// path the push fan-out uses — dedup + snapshot projection unchanged).
///
/// Returns `true` only when the feed's visible row set advanced. A feed can
/// admit an event mechanically but suppress it, dedup it, or use it only as
/// hidden attribution; those rows must not count as pull-page progress.
pub type FeedApply = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Grow the feed's render viewport by one page, called after a drained page is
/// ingested. Return value (if any) is ignored by the controller.
pub type FeedAdvance = Arc<dyn Fn() + Send + Sync>;

/// Evict a single superseded source from the feed's visible state, by its
/// lowercase-hex event id. The controller calls this for each `LogOp::Replaced`
/// row a drain surfaces (a replaceable event's prior version) so the stale
/// version stops rendering; the superseding version is applied through `apply`
/// like any other positive row. In production this is the feed's own
/// source-removal path (e.g. `FlatFeed::remove_source`).
pub type FeedReplace = Arc<dyn Fn(&str) + Send + Sync>;

/// Clear the feed's visible state for a perspective change (account switch,
/// follow-set replacement, WoT preset change) and report whether anything was
/// actually cleared. In production this is `FlatFeed::reset_for_perspective_change`.
///
/// [`PullFeedController::reset`] calls this exactly once. The pager lock is
/// released *before* this hook is called — sequential, not atomic — to prevent
/// a deadlock if the hook acquires any internal lock. Callers must invoke
/// `reset` from the same serialized perspective-update path as `load_older`
/// so no concurrent `load_older` can observe the rewound cursor before the
/// visible state is cleared.
pub type FeedReset = Arc<dyn Fn() -> bool + Send + Sync>;

/// A [`FeedInterestShape`] backed by a closure, so a feed's live interest (e.g.
/// the active account's follow set + compiled acquisition kinds) is re-evaluated on
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

/// A live source that may compile to several non-mergeable shapes.
///
/// This is required when a single feed declaration expands to multiple
/// relay-pinned interests. Returning an empty vector fails closed.
pub trait FeedInterestShapes: Send + Sync {
    fn interest_shapes(&self) -> Vec<InterestShape>;
}

/// A [`FeedInterestShapes`] backed by a closure.
pub struct ClosureInterestShapes<F>(F);

impl<F> ClosureInterestShapes<F>
where
    F: Fn() -> Vec<InterestShape> + Send + Sync,
{
    /// Wrap a closure as a fail-closed multi-shape interest provider.
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> FeedInterestShapes for ClosureInterestShapes<F>
where
    F: Fn() -> Vec<InterestShape> + Send + Sync,
{
    fn interest_shapes(&self) -> Vec<InterestShape> {
        (self.0)()
    }
}

enum PullProvider {
    One(Arc<dyn FeedInterestShape + Send + Sync>),
    Many(Arc<dyn FeedInterestShapes + Send + Sync>),
}

impl PullProvider {
    fn pull_scope(&self) -> Option<PullScope> {
        match self {
            Self::One(provider) => provider.interest_shape().map(PullScope::InterestShape),
            Self::Many(provider) => {
                let mut shapes = Vec::new();
                for shape in provider.interest_shapes() {
                    if !shapes.contains(&shape) {
                        shapes.push(shape);
                    }
                }
                match shapes.len() {
                    0 => None,
                    1 => shapes.pop().map(PullScope::InterestShape),
                    _ => Some(PullScope::InterestShapes(shapes)),
                }
            }
        }
    }
}

/// The pull-backed [`FeedController`]. Owns the feed's pager (seq cursor) and the
/// injected closures; see the module docs for the `load_older` contract.
///
/// `replace` and `reset` are optional perspective-change hooks (see
/// [`FeedReplace`] / [`FeedReset`]): `new` wires neither, `new_with_replacement`
/// wires `replace`, and `new_with_perspective` wires both.
pub struct PullFeedController {
    pager: Mutex<FeedPullPager>,
    provider: PullProvider,
    pull: PullFn,
    apply: FeedApply,
    replace: Option<FeedReplace>,
    reset: Option<FeedReset>,
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
    ///
    /// No perspective hooks are wired: replaceable supersession is not evicted
    /// from the visible window and [`reset`](FeedController::reset) only rewinds
    /// the cursor. Use [`new_with_replacement`](Self::new_with_replacement) or
    /// [`new_with_perspective`](Self::new_with_perspective) for those.
    #[must_use]
    pub fn new(
        provider: Arc<dyn FeedInterestShape + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        advance: FeedAdvance,
    ) -> Arc<Self> {
        Self::new_with_perspective(provider, pull, apply, None, None, advance)
    }

    /// Construct a controller that also evicts replaceable supersession from the
    /// visible window. Identical to [`new`](Self::new) except the drain calls
    /// `replace` for every `LogOp::Replaced` row it surfaces, removing the prior
    /// version of a replaceable event so only the current one renders.
    #[must_use]
    pub fn new_with_replacement(
        provider: Arc<dyn FeedInterestShape + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        replace: FeedReplace,
        advance: FeedAdvance,
    ) -> Arc<Self> {
        Self::new_with_perspective(provider, pull, apply, Some(replace), None, advance)
    }

    /// Construct a controller with both optional perspective hooks. `replace`
    /// evicts replaceable supersession during a drain; `reset` clears the
    /// visible window on a perspective change. The reset/replace hooks are
    /// mechanics-only — they name no app primary-kind policy (D0) and hydrate no
    /// secondary data (D11); the closures the caller injects own that.
    #[must_use]
    pub fn new_with_perspective(
        provider: Arc<dyn FeedInterestShape + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        replace: Option<FeedReplace>,
        reset: Option<FeedReset>,
        advance: FeedAdvance,
    ) -> Arc<Self> {
        Self::new_with_perspective_and_window_policy(
            provider,
            pull,
            apply,
            replace,
            reset,
            advance,
            FeedWindowPolicy::default(),
        )
    }

    /// Construct a controller with perspective hooks and explicit pull budgets
    /// from the app-declared feed window policy.
    #[must_use]
    pub fn new_with_perspective_and_window_policy(
        provider: Arc<dyn FeedInterestShape + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        replace: Option<FeedReplace>,
        reset: Option<FeedReset>,
        advance: FeedAdvance,
        window_policy: FeedWindowPolicy,
    ) -> Arc<Self> {
        Self::new_with_provider(
            PullProvider::One(provider),
            pull,
            apply,
            replace,
            reset,
            advance,
            window_policy,
        )
    }

    /// Construct a controller whose live row source may expand to multiple
    /// non-mergeable shapes.
    #[must_use]
    pub fn new_with_shape_set(
        provider: Arc<dyn FeedInterestShapes + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        replace: Option<FeedReplace>,
        reset: Option<FeedReset>,
        advance: FeedAdvance,
    ) -> Arc<Self> {
        Self::new_with_shape_set_and_window_policy(
            provider,
            pull,
            apply,
            replace,
            reset,
            advance,
            FeedWindowPolicy::default(),
        )
    }

    /// Construct a multi-shape controller with explicit pull budgets from the
    /// app-declared feed window policy.
    #[must_use]
    pub fn new_with_shape_set_and_window_policy(
        provider: Arc<dyn FeedInterestShapes + Send + Sync>,
        pull: PullFn,
        apply: FeedApply,
        replace: Option<FeedReplace>,
        reset: Option<FeedReset>,
        advance: FeedAdvance,
        window_policy: FeedWindowPolicy,
    ) -> Arc<Self> {
        Self::new_with_provider(
            PullProvider::Many(provider),
            pull,
            apply,
            replace,
            reset,
            advance,
            window_policy,
        )
    }

    fn new_with_provider(
        provider: PullProvider,
        pull: PullFn,
        apply: FeedApply,
        replace: Option<FeedReplace>,
        reset: Option<FeedReset>,
        advance: FeedAdvance,
        window_policy: FeedWindowPolicy,
    ) -> Arc<Self> {
        // Cursor-only pager — no initial shape check. The live shape is read on
        // every load_older call, so a controller registered before sign-in
        // becomes active as soon as the provider yields a real shape.
        let pager = FeedPullPager::at_start().with_budgets(
            window_policy.source_page_size(),
            window_policy.source_scan_budget(),
        );
        Arc::new(Self {
            pager: Mutex::new(pager),
            provider,
            pull,
            apply,
            replace,
            reset,
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
        self.load_older_status().changed
    }

    fn load_older_status(&self) -> FeedLoadStatus {
        // Live, fail-closed interest. If the feed can no longer express a covered
        // shape (e.g. logout cleared the follow set), do nothing — never scan.
        let Some(scope) = self.provider.pull_scope() else {
            return FeedLoadStatus::unchanged(FeedLoadStopReason::SourceUnavailable);
        };

        let mut accepted = 0usize;
        let mut visited = 0usize;
        let mut replaced = false;
        let (page_size, scan_budget) = {
            let Ok(pager) = self.pager.lock() else {
                return FeedLoadStatus::session_unavailable();
            };
            (pager.page_size(), pager.scan_budget())
        };

        let reason = loop {
            // Bounded seq-ordered drain. The pager owns the cursor and the
            // budget; it terminates on PageFilled / Exhausted / Gap /
            // ScanBudget — it never polls.
            let outcome = {
                let Ok(mut pager) = self.pager.lock() else {
                    return FeedLoadStatus::session_unavailable();
                };
                let pull = &self.pull;
                pager.drain(|after_seq, limits| pull(scope.clone(), after_seq, limits))
            };
            let stop_reason = outcome.stop.into();
            visited = visited.saturating_add(outcome.visited);

            // Evict any superseded sources FIRST so the stale version never
            // lingers alongside its replacement, then apply the page through
            // the feed's OWN ingest path. A `Gap` already rebased the pager
            // cursor explicitly; any rows drained before it are still applied.
            if let Some(replace) = self.replace.as_ref() {
                for replaced_id in &outcome.replaced_ids {
                    replace(replaced_id);
                }
            }
            replaced |= !outcome.replaced_ids.is_empty();
            for event in &outcome.events {
                if (self.apply)(event) {
                    accepted = accepted.saturating_add(1);
                }
            }

            let should_continue = matches!(outcome.stop, crate::DrainStop::PageFilled)
                && accepted < page_size
                && visited < scan_budget;
            if !should_continue {
                break stop_reason;
            }
        };

        let progressed = accepted > 0 || replaced;
        if progressed {
            (self.advance)();
            FeedLoadStatus::changed(reason)
        } else {
            FeedLoadStatus::unchanged(reason)
        }
    }

    fn replace_source(&self, source_id: &str) -> bool {
        // External, key-driven eviction (`FeedRegistry::replace`): hand the
        // source id straight to the wired replace hook so the feed drops that
        // source from its visible window. This is the SAME hook the drain calls
        // for `LogOp::Replaced` rows (see `load_older`); the only difference is
        // who supplies the id — there the pager derives it, here the caller does.
        // With no hook wired the controller fails closed (`false`): it has no
        // way to evict and never fabricates acceptance, so a key-driven replace
        // against a `new`-constructed controller is an honest no-op.
        match self.replace.as_ref() {
            Some(replace) => {
                replace(source_id);
                true
            }
            None => false,
        }
    }

    fn reset(&self) -> bool {
        // Perspective change: two sequential steps, not one atomic operation.
        //
        // Step 1 — rewind the pull cursor under the pager lock so the next
        // load_older replays from seq 0 under the new perspective.
        //
        // Step 2 — release the pager lock, THEN call FeedReset to clear the
        // visible state. The lock must be dropped first: holding it across the
        // hook would risk a deadlock if the hook acquires any lock of its own.
        //
        // Because the two steps are not atomic, callers must invoke reset from
        // the same serialized feed/perspective-update path as load_older — a
        // concurrent load_older between step 1 and step 2 would observe the
        // cursor at seq 0 with the old visible window and double-replay rows.
        // The host contract is serialization, not cross-thread isolation.
        let Ok(mut pager) = self.pager.lock() else {
            // Poisoned pager: fail closed — no rewind, no visible clear.
            return false;
        };
        pager.rewind(); // step 1
        drop(pager); // release before calling external hook (step 2)
        self.reset.as_ref().map(|reset| reset()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests;
