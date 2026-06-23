//! Feed-management `impl NmpApp` methods — extracted from `lib.rs` to keep
//! each file under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `register_feed`, `load_older_feed`, `open_interest`,
//! `close_interest`, `register_feed_with_observer`,
//! `open_observed_interest`, `open_observed_interest_pinned`,
//! `close_interest_pinned`, `unregister_feed`.

use std::sync::Arc;

use nmp_core::__ffi_internal::{register_rust_observer, register_rust_observer_muted};
use nmp_core::actor::ActorCommand;
use nmp_core::InterestsCommand;
use nmp_core::{KernelEventObserver, KernelEventObserverId};

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
    pub fn load_older_feed(&self, key: &str) -> bool {
        let changed = self.feed_registry.load_older(key);
        if changed {
            self.mark_changed_since_emit();
        }
        changed
    }

    /// Register (or attach an owner to) a generic tailing feed interest.
    ///
    /// Typed wrapper for [`ActorCommand::OpenInterest`]. The caller supplies a
    /// verbatim NIP-01 REQ filter JSON; the kernel parses it into an
    /// `InterestShape` and refcounts by `(filter, consumer_id, scope)`.
    ///
    /// * `scope` — `0` = `ActiveAccount` (re-route on account switch),
    ///   `1` = `Global` (account-agnostic).
    ///
    /// D6: a malformed filter is a no-op (the caller should validate first via
    /// `InterestShape::from_filter_json` and surface a toast if needed).
    pub fn open_interest(&self, filter_json: String, consumer_id: String, scope: u32) {
        self.send_cmd(ActorCommand::Interests(InterestsCommand::OpenInterest {
            filter_json,
            consumer_id,
            scope,
        }));
    }

    /// Detach one owner from an interest registered via [`Self::open_interest`].
    ///
    /// Typed wrapper for [`ActorCommand::CloseInterest`] with `relay_pin: None`
    /// (the normal outbox-routed path). For relay-pinned closes use
    /// [`Self::close_interest_pinned`].
    ///
    /// The `(filter_json, consumer_id, scope)` triple MUST match the open call
    /// so the reconstructed `InterestShape` hash lands on the same registry
    /// slot. D6: a close of a non-existent slot is harmless.
    pub fn close_interest(&self, filter_json: String, consumer_id: String, scope: u32) {
        self.send_cmd(ActorCommand::Interests(InterestsCommand::CloseInterest {
            filter_json,
            consumer_id,
            scope,
            relay_pin: None,
        }));
    }

    /// Register a **transient** feed surface — a feed whose snapshot key must
    /// be torn down when its screen closes (a visited profile / open thread),
    /// as opposed to [`Self::register_feed`]'s permanent feeds (the home
    /// feed).
    ///
    /// This does everything `register_feed` does — registers the
    /// [`nmp_feed::FeedController`] under `key` in the feed registry (the
    /// render payload is emitted by a separately-registered typed snapshot
    /// projection, e.g. `register_typed_feed_sidecar`, not by this call) — AND
    /// additionally installs `observer` into the kernel's
    /// [`KernelEventObserver`] registry in **muted** state (ADR-0062).  The
    /// observer will NOT fire from the global fan-out until the caller passes
    /// the returned id to [`Self::open_observed_interest`], which replays the
    /// in-memory read-cache (and, for explicit `ids`-bearing shapes, the durable
    /// store) to the observer and then activates it.  The caller typically
    /// passes the same `Arc<FlatFeed>` as both `controller` and `observer`.
    ///
    /// Registering the same `key` twice replaces the controller / projection
    /// (last-writer-wins) and revokes the previously-tracked observer before
    /// installing the new one, so a re-open never leaks the prior observer.
    ///
    /// D6 — a poisoned bookkeeping mutex degrades to "observer registered but
    /// untracked": the feed still works, but its observer outlives the screen
    /// (a bounded soft-leak, never a crash). D8 — init-style registry push.
    #[must_use = "pass the returned id to open_observed_interest for catch-up"]
    pub fn register_feed_with_observer(
        &self,
        key: impl Into<String>,
        controller: Arc<dyn nmp_feed::FeedController>,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        let key = key.into();
        self.register_feed(key.clone(), controller);
        // ADR-0062: register muted so the observer doesn't receive events
        // from the global fan-out until the replay+activate step completes.
        let observer_id = register_rust_observer_muted(&self.event_observers, observer);
        if let Ok(mut map) = self.interest_feed_observers.lock() {
            if let Some(previous) = map.insert(key, observer_id) {
                // A re-open under the same key: the new observer is now
                // tracked; revoke the stale one so the kernel stops fanning
                // events into the replaced feed instance.
                self.unregister_event_observer(previous);
            }
        }
        observer_id
    }

    /// ADR-0062 — open an interest with read-model catch-up replay to the
    /// muted observer identified by `observer_id`, then activate it.
    ///
    /// Validates the filter JSON via `InterestShape::from_filter_json` and
    /// sends `ActorCommand::OpenObservedInterest`. A malformed filter emits a
    /// toast (same as `nmp_app_open_interest`) and returns without sending.
    ///
    /// `replay_shapes` are the `InterestShape`s used to match events in the
    /// kernel's read-cache during replay. These may differ from the filter
    /// (e.g. a thread feed uses two shapes: `#e` replies + root-by-id).
    pub fn open_observed_interest(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        observer_id: KernelEventObserverId,
        replay_shapes: Vec<nmp_planner::InterestShape>,
        replay_limit: usize,
    ) {
        self.open_observed_interest_pinned(
            filter_json,
            consumer_id,
            scope,
            None,
            observer_id,
            replay_shapes,
            replay_limit,
        );
    }

    /// ADR-0062 + relay-pin — the [`Self::open_observed_interest`] variant that
    /// routes the interest to exactly one relay (the planner's relay-pin lane).
    ///
    /// `relay_pin` — `Some(host)` pins the interest to that relay, bypassing
    /// NIP-65 outbox routing; `None` is identical to `open_observed_interest`.
    /// NIP-50 search (`nmp_app_search_open`) opens one pinned interest per
    /// resolved search relay. The pin participates in the `InterestShape` hash,
    /// so the matching close MUST pass the same pin (see
    /// [`Self::close_interest_pinned`]).
    #[allow(clippy::too_many_arguments)]
    pub fn open_observed_interest_pinned(
        &self,
        filter_json: &str,
        consumer_id: &str,
        scope: u32,
        relay_pin: Option<String>,
        observer_id: KernelEventObserverId,
        replay_shapes: Vec<nmp_planner::InterestShape>,
        replay_limit: usize,
    ) {
        // Validate filter — same guard as nmp_app_open_interest.
        if nmp_planner::InterestShape::from_filter_json(filter_json).is_none() {
            // D6: invalid filter is a no-op.
            return;
        }
        self.send_cmd(ActorCommand::Interests(InterestsCommand::OpenObservedInterest {
            filter_json: filter_json.to_string(),
            consumer_id: consumer_id.to_string(),
            scope,
            relay_pin,
            observer_id,
            replay_shapes,
            replay_limit,
        }));
    }

    /// Send a relay-pinned `CloseInterest` matching a
    /// [`Self::open_observed_interest_pinned`] open. The `(filter_json,
    /// consumer_id, scope, relay_pin)` tuple MUST match the open so the
    /// reconstructed `InterestShape` hash lands on the same registry slot.
    pub fn close_interest_pinned(
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

    /// Tear down a feed registered through [`Self::register_feed_with_observer`].
    ///
    /// Performs all three removals the registration installed, in any
    /// combination present (each is an independent no-op when its target is
    /// absent, so an unknown key is harmless):
    ///
    /// 1. the [`nmp_feed::FeedController`] from the feed registry;
    /// 2. the snapshot projection closure (generic + typed) so it stops
    ///    emitting a stale empty subtree on every tick;
    /// 3. the tracked [`KernelEventObserver`], if one was recorded for `key`.
    ///
    /// CALLER CONTRACT — call this ONLY for transient keys registered through
    /// [`Self::register_feed_with_observer`]. It is **destructive on any key**
    /// that has a live `FeedController` / projection: calling it on the
    /// permanent home-feed key (`nmp.feed.home`, registered via the plain
    /// [`Self::register_feed`]) WOULD drop the home feed's controller and
    /// projection — it is "safe" there only in the sense that it never panics,
    /// not that it preserves the feed. The home feed has no tracked observer, so
    /// step 3 is a no-op there, but steps 1–2 are not.
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
        let removed_observer = self
            .interest_feed_observers
            .lock()
            .ok()
            .and_then(|mut map| map.remove(key));
        let removed_any = removed_feed || removed_projection || removed_observer.is_some();
        if let Some(observer_id) = removed_observer {
            self.unregister_event_observer(observer_id);
        }
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

    /// #1740 step 2 — clone the kernel-event-observer registry slot.
    ///
    /// Captured into a feed-session teardown closure to revoke the session's
    /// ingest observer by id on `close_feed`. Same slot
    /// [`Self::register_event_observer`] writes into (D4).
    #[must_use]
    pub fn event_observers_handle(&self) -> nmp_core::__ffi_internal::KernelEventObserverSlot {
        Arc::clone(&self.event_observers)
    }

    /// ADR-0058 §8 step-6B — the in-process pull seam a feed's
    /// [`nmp_feed::PullFeedController`] drains on `load_older`.
    ///
    /// Returns a plain Rust closure `(scope, after_seq) -> page` that reads the
    /// kernel's published [`EventStore`](nmp_store::EventStore) directly
    /// via [`nmp_core::pull_page_over`]. This is **not** a new C-ABI symbol and
    /// **not** a projection accessor: it reads the raw ingest log exactly as the
    /// existing [`crate::pull::nmp_app_pull_page`] door does (ADR-0039 §6.1
    /// preserved — no host projection-pull accessor is added). The composition
    /// root hands this to `PullFeedController`; the host never sees it.
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
