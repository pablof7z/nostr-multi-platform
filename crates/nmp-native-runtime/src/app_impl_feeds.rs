//! Feed-management `impl NmpApp` methods — extracted from `lib.rs` to keep
//! each file under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `register_feed`, internal feed paging, internal observed-interest
//! wiring, `close_interest_pinned`, `unregister_feed`, and the
//! [`ObservedProjectionRegistrar`] impl.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
use nmp_core::substrate::{ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionId;

use crate::app_struct::NmpApp;

impl NmpApp {
    /// Register a reusable feed surface. The controller owns ordering,
    /// viewport state, paging, and render payload selection; native shells
    /// only render the emitted projection and report viewport intent.
    pub fn register_feed(
        &self,
        key: impl Into<String>,
        controller: Arc<dyn nmp_feed::FeedController>,
    ) {
        let key = key.into();
        self.feed_registry
            .register(key.clone(), Arc::clone(&controller));
    }

    #[must_use]
    pub(crate) fn load_older_feed_by_key(&self, key: &str) -> bool {
        let changed = self.feed_registry.load_older(key);
        if changed {
            self.mark_changed_since_emit();
        }
        changed
    }

    /// ADR-0062 + relay-pin — open an observed interest and route it to exactly
    /// one relay (the planner's relay-pin lane).
    ///
    /// `relay_pin` — `Some(host)` pins the interest to that relay, bypassing
    /// NIP-65 outbox routing; `None` leaves routing unpinned.
    /// NIP-50 search sessions open one pinned interest per
    /// resolved search relay. The pin participates in the `InterestShape` hash,
    /// so the matching close MUST pass the same pin (see
    /// [`Self::close_interest_pinned`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open_observed_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
        observer_id: ObservedProjectionId,
        replay_shapes: Vec<nmp_planner::InterestShape>,
        replay_limit: usize,
    ) {
        // Validate filter before handing the raw acquisition request to the
        // actor; this is internal runtime machinery, not a public raw-interest
        // app door.
        if nmp_planner::InterestShape::from_filter_json(filter_json).is_none() {
            // D6: invalid filter is a no-op.
            return;
        }
        self.send_cmd(ActorCommand::Interests(
            InterestsCommand::OpenObservedInterest {
                filter_json: filter_json.to_string(),
                consumer_id: consumer_id.to_string(),
                scope,
                relay_pin,
                observer_id,
                replay_shapes,
                replay_limit,
            },
        ));
    }

    /// Send a relay-pinned `CloseInterest` matching a
    /// [`Self::open_observed_interest_pinned`] open. The `(filter_json,
    /// consumer_id, scope, relay_pin)` tuple MUST match the open so the
    /// reconstructed `InterestShape` hash lands on the same registry slot.
    pub(crate) fn close_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
    ) {
        self.send_cmd(ActorCommand::Interests(InterestsCommand::CloseInterest {
            filter_json: filter_json.to_string(),
            consumer_id: consumer_id.to_string(),
            scope,
            relay_pin,
        }));
    }

    /// Tear down a feed registered via [`Self::register_feed`].
    ///
    /// Performs both removals the registration installed, in any combination
    /// present (each is an independent no-op when its target is absent, so an
    /// unknown key is harmless):
    ///
    /// 1. the [`nmp_feed::FeedController`] from the feed registry;
    /// 2. the snapshot projection closure (generic + typed), plus the feed's
    ///    author provider, so it stops emitting a stale empty subtree per tick.
    ///
    /// CALLER CONTRACT — this is **destructive on any key** that has a live
    /// `FeedController` / projection, including app-owned long-lived feed keys
    /// such as `app.feed.following`: calling it there WOULD drop that feed's
    /// controller and projection. It is "safe" only in the sense that it never
    /// panics, not that it preserves the feed.
    ///
    /// Returns `true` when any registration was removed. D6 — poisoned locks
    /// degrade to partial teardown (best-effort); the `nmp_app_free` actor
    /// join remains the hard fence for in-flight callbacks.
    pub fn unregister_feed(&self, key: &str) -> bool {
        let removed_feed = self.feed_registry.unregister(key);
        // ADR-0063 D7 (#1671 Lane H) — remove this feed's typed projection AND
        // its feed-author provider in the same lock.
        let removed_projection = self
            .snapshot_projections
            .lock()
            .map(|mut registry| {
                let removed_proj = registry.remove(key);
                let removed_provider = registry.remove_feed_author_provider(key);
                removed_proj || removed_provider
            })
            .unwrap_or(false);
        let removed_any = removed_feed || removed_projection;
        if removed_any {
            self.mark_changed_since_emit();
        }
        removed_any
    }

    /// Remove a typed snapshot projection registered under `key`.
    ///
    /// Idempotent: an unknown key is a silent no-op (D6). Signals a
    /// `MarkChangedSinceEmit` so the next snapshot tick reflects the removal
    /// (the stale projection stops emitting its subtree).
    pub fn remove_snapshot_projection(&self, key: &str) {
        let removed = self
            .snapshot_projections
            .lock()
            .map(|mut registry| registry.remove(key))
            .unwrap_or(false);
        if removed {
            self.mark_changed_since_emit();
        }
    }

    /// #1740 step 2 — clone the feed-controller registry slot.
    ///
    /// A feed-session compiler captures this `Arc` into a `Send` teardown
    /// closure so `close_feed` can `unregister` the session's controller
    /// without holding `&NmpApp`. The slot is the SAME registry
    /// [`Self::register_feed`] writes into (single source of truth — D4).
    #[must_use]
    pub fn feed_registry_handle(&self) -> nmp_feed::FeedRegistrySlot {
        Arc::clone(&self.feed_registry)
    }

    /// #1740 step 2 — clone the snapshot-projection registry slot.
    ///
    /// Captured into a feed-session teardown closure to remove the session's
    /// typed sidecar projection on `close_feed`. Same slot
    /// [`Self::register_typed_snapshot_projection`] writes into.
    #[must_use]
    pub fn snapshot_projections_handle(&self) -> nmp_core::__ffi_internal::SnapshotProjectionSlot {
        Arc::clone(&self.snapshot_projections)
    }

    /// #1740 step 2 — clone the observed-projection sink registry slot.
    ///
    /// Captured into a feed-session teardown closure to revoke the session's
    /// observed-projection sink by id on `close_feed`. Same slot
    /// [`Self::open_observed_projection`] writes into (D4).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn event_observers_handle(
        &self,
    ) -> nmp_core::__ffi_internal::ObservedProjectionSinkSlot {
        Arc::clone(&self.event_observers)
    }

    /// Test-support observer count for white-box runtime invariants.
    ///
    /// Exposes only the aggregate count, not the raw observer slot handle.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_observed_projection_sink_count(&self) -> usize {
        nmp_core::__ffi_internal::rust_observer_count(&self.event_observers)
    }

    /// ADR-0058 §8 step-6B — the in-process pull seam a feed's
    /// [`nmp_feed::PullFeedController`] drains on `load_older`.
    ///
    /// Returns a plain Rust closure `(scope, after_seq) -> page` that reads the
    /// kernel's published [`EventStore`](nmp_store::EventStore) directly
    /// via [`nmp_core::pull_page_over`]. This is **not** a C-ABI symbol and
    /// **not** a projection accessor: it reuses the same raw ingest-log pull
    /// machinery exposed to hosts only through the typed UniFFI
    /// `NmpApp::mirror_pull_page` surface (ADR-0039 §6.1 preserved — no host
    /// projection-pull accessor is added). The composition root hands this to
    /// `PullFeedController`; the host never sees it.
    #[must_use]
    pub fn feed_pull_fn(&self) -> nmp_feed::PullFn {
        use nmp_core::{pull_page_over, PullLimits};
        use nmp_store::{PullPage, ScanLogResult};
        use std::num::NonZeroUsize;

        let slot = Arc::clone(&self.read_handles.event_store_handle);
        let max_entries =
            NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let max_scan = NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE.saturating_mul(8))
            .unwrap_or(NonZeroUsize::MIN);
        let limits = PullLimits {
            max_entries,
            max_scan_entries: max_scan,
        };

        Arc::new(move |scope: nmp_core::PullScope, after_seq: u64| {
            // Fail-closed terminator: empty exhausted page stops the drain.
            let exhausted = || {
                ScanLogResult::Page(PullPage {
                    entries: Vec::new(),
                    next_after_seq: after_seq,
                    latest_seq: after_seq,
                    has_more: false,
                })
            };
            let store = {
                let Ok(guard) = slot.lock() else {
                    return exhausted();
                };
                match guard.as_ref() {
                    Some(s) => Arc::clone(s),
                    None => return exhausted(),
                }
            }; // slot lock released before the store read
            match pull_page_over(store.as_ref(), scope, after_seq, limits) {
                Ok(result) => result,
                Err(_) => exhausted(),
            }
        })
    }
}

// ── ObservedProjectionRegistrar ───────────────────────────────────────────────

impl ObservedProjectionRegistrar for NmpApp {
    /// Open a single observed-projection session.
    ///
    /// Combines the three steps that were previously open-coded in each
    /// caller:
    ///
    /// 1. `register_rust_observer_muted` — installs `decl.observer` in MUTED
    ///    state so the kernel fan-out does not deliver events until activation.
    /// 2. `open_observed_interest_pinned` — sends `OpenObservedInterest` to the
    ///    actor, which replays the in-memory read-cache to the observer and then
    ///    activates it (unmutes the slot and hooks it into the live fan-out).
    /// 3. Returns the `ObservedProjectionId` (the observer's slot id), which
    ///    the caller passes to [`close_observed_projection`] for cleanup.
    ///
    /// The close params are recorded in `observed_projection_sessions` so
    /// `close_observed_projection` can reverse both registrations (observer +
    /// interest) from just the id.
    ///
    /// D6 fail-closed on two poison paths:
    ///
    /// * `register_rust_observer_muted` returns the reserved sentinel
    ///   `ObservedProjectionId(0)` when the observer slot mutex is poisoned. A
    ///   real id is always `>= 1` (the allocator starts at 1). On the sentinel
    ///   we do NOT track the session and do NOT open the interest — opening
    ///   against id 0 would route fan-out to an unaddressable slot, and tracking
    ///   it would collapse every poisoned open onto the single `0` key so the
    ///   first close leaks every other session's interests. We return id 0; the
    ///   caller treats it as "no session opened".
    /// * a declaration with no concrete shape is rejected before registration.
    ///   Production app read models must not recreate the deleted filterless
    ///   all-event observer path through an empty observed projection.
    /// * a poisoned `observed_projection_sessions` mutex means we cannot record
    ///   the close params, so `close_observed_projection` could never reverse
    ///   this open. Rather than leak the just-registered observer + a live
    ///   interest, we unregister the observer and return id 0 without opening.
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed_projection_handle().open(decl)
    }

    /// Close a single observed-projection session opened via
    /// [`open_observed_projection`].
    ///
    /// Reverses both registrations in order:
    ///
    /// 1. Closes the pinned interest (sends `CloseInterest` to the actor).
    /// 2. Unregisters the observer from the kernel fan-out.
    ///
    /// Idempotent: closing an unknown or already-closed id is a harmless no-op
    /// (D6 — the sessions map lookup returns `None`).
    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.observed_projection_handle().close(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn nmp_core::substrate::ObservedProjectionRegistrar + Send + Sync> {
        Arc::new(self.observed_projection_handle())
    }
}
