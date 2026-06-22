//! PR-4 composition seams for `KernelReducer`.
//!
//! These four methods let a wasm32 composition root wire the OP-feed engine
//! into the kernel without depending on `NmpApp` (which lives in `nmp-ffi`,
//! not available on wasm32) or the native actor thread:
//!
//! * `register_event_observer` — wire a `KernelEventObserver` into the fan-out slot.
//! * `register_typed_snapshot_projection` — wire a typed FlatBuffers projection.
//! * `active_account_handle` — read the active-account pubkey slot.
//! * `event_store_handle` — read the kernel event-store `Arc`.
//!
//! All methods delegate either to `self.kernel` (for slot handles that are
//! already `pub` there) or to `self.observer_slot` / `self.snapshot_slot`
//! (the per-reducer slots initialised in `KernelReducer::new`).
//!
//! # Doctrine
//!
//! * **D0** — surface types are all substrate-level: `Arc<dyn EventStore>`,
//!   `ActiveAccountSlot`, `KernelEventObserver`, `TypedProjectionData`.
//!   No NIP or app nouns.
//! * **D6** — poisoned mutex on register/lookup is a silent no-op; the
//!   caller never panics.
//! * **D8** — all methods are O(n-observers) at worst; no I/O, no blocking.

use std::sync::Arc;

use crate::actor::register_rust_observer;
use crate::slots::ActiveAccountSlot;
use crate::store::EventStore;
use crate::substrate::{ContactsLookup, IngestParser, ProfileLookup};
use crate::{KernelEventObserver, KernelEventObserverId, TypedProjectionData};

impl super::KernelReducer {
    // ── Event-observer slot seam ──────────────────────────────────────────

    /// Register an in-process Rust observer that will be called for every
    /// event the kernel accepts (i.e. returns `Inserted` or `Replaced` from
    /// `EventStore::insert`).
    ///
    /// Returns an opaque [`KernelEventObserverId`] the caller retains to
    /// unregister later. Registration is idempotent: the same `Arc` can be
    /// registered multiple times and fires once per registration.
    ///
    /// This is the wasm32 equivalent of `NmpApp::register_event_observer`.
    pub fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        register_rust_observer(&self.observer_slot, observer)
    }

    // ── Typed snapshot-projection seam ───────────────────────────────────

    /// Register a typed FlatBuffers snapshot projection under `key`.
    ///
    /// The closure `f` is called once per `make_update_frame` tick (on the
    /// wasm32 path that is the 1 Hz timer + explicit snapshot pulls). It
    /// returns `Some(TypedProjectionData)` when there is a changed payload to
    /// emit, or `None` to omit an unchanged row for that tick. Under
    /// incremental apply, omission retains the host cache; unregistering the
    /// key emits an explicit `Cleared` row.
    ///
    /// This is the wasm32 equivalent of
    /// `NmpApp::register_typed_snapshot_projection`.
    pub fn register_typed_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_typed(key, f);
        }
        // Poisoned mutex: D6 silent fail. The projection simply never
        // appears in snapshots — same graceful-degrade as a missing
        // registration.
    }

    // ── Kernel handle pass-throughs ───────────────────────────────────────

    /// Return the kernel's active-account pubkey slot.
    ///
    /// The returned [`ActiveAccountSlot`] is `Arc<Mutex<Option<String>>>`.
    /// Composition roots pass it to `ActiveFollowSet::new` (rung 4) so the
    /// follow-set producer can seed itself and respond to account switches
    /// without holding a reference to the full reducer.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        self.kernel.active_account_handle()
    }

    /// Return the kernel's event-store handle.
    ///
    /// Used by the composition root to build an `EventLookup` closure
    /// (`Arc<dyn Fn(&str) -> Option<KernelEvent> + Send + Sync>`) for
    /// `register_op_feed`.  The returned `Arc<dyn EventStore>` is `Send +
    /// Sync`, so the closure can be stored across ticks without holding a
    /// `KernelReducer` borrow.
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        self.kernel.event_store_handle()
    }

    /// Install the profile lookup used by kernel profile readers.
    ///
    /// Wasm composition roots cannot go through the native `AppHost`
    /// `set_profile_lookup` seam, so they use this method to share one
    /// profile cache between the kind:0 ingest parser and kernel readers.
    pub fn set_profile_lookup(&mut self, lookup: Arc<dyn ProfileLookup>) {
        self.kernel.set_profile_lookup(lookup);
    }

    /// Install the contacts lookup used by kernel follow-feed readers.
    ///
    /// Wasm composition roots cannot go through the native `AppHost`
    /// `set_contacts_lookup` seam, so they use this method to share one
    /// contacts cache between the kind:3 ingest parser and kernel readers.
    pub fn set_contacts_lookup(&mut self, lookup: Arc<dyn ContactsLookup>) {
        self.kernel.set_contacts_lookup(lookup);
    }

    /// Register a post-store ingest parser against the wrapped kernel.
    ///
    /// Mirrors `NmpApp::register_ingest_parser` for reducer-owned wasm
    /// compositions. Parsers registered here fire on the same
    /// `project_accepted_event` path as native ingest.
    pub fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>) {
        self.kernel.register_ingest_parser(kind, parser);
    }

    /// Snapshot the active account's timeline-author projection.
    ///
    /// This is a reducer-owned wrapper around the kernel accessor so wasm
    /// composition tests can assert the follow-feed planner projection without
    /// reaching through private kernel state.
    #[must_use]
    pub fn active_timeline_authors(&self) -> Vec<String> {
        self.kernel.active_timeline_authors()
    }

    // ── Composition-root wiring (moved here for the LOC ceiling, #1753) ──────

    /// Populate the kernel's configured-relay lanes from a caller-supplied
    /// list of `(url, role)` pairs.
    ///
    /// Each `role` string is canonicalised via the kernel's own
    /// `canonical_relay_role` pass (same normalisation the native actor
    /// applies on every relay-edit write). [`crate::kernel::AppRelay`] is
    /// `pub(crate)`; external crates (e.g. `nmp-wasm`) pass raw string
    /// pairs and let this method build the typed rows internally.
    ///
    /// Calling this before the first `make_update_frame` ensures the
    /// `relay_statuses` Tier-3 rows and the `configured_relays` typed
    /// projection both carry real URLs rather than empty defaults.
    pub fn set_configured_relays(&mut self, rows: Vec<(String, String)>) {
        use crate::kernel::AppRelay;
        let relay_rows: Vec<AppRelay> = rows
            .into_iter()
            .map(|(url, role)| AppRelay::new(url, role))
            .collect();
        self.kernel.set_configured_relays(relay_rows);
    }

    /// Install the production outbox-routing substrate on the wrapped kernel.
    ///
    /// The wasm32 composition has no `AppHost` / actor `set_routing_substrate`
    /// seam (that path is native-only), so the chirp-web composition root calls
    /// this directly to swap the kernel's default no-op
    /// [`crate::substrate::EmptyOutboxRouter`] for the production
    /// `nmp_router::GenericOutboxRouter` + a `MailboxCache`. Without this every
    /// outbox-direction REQ (kind:0 profile claims, kind:3 contacts, kind:10002
    /// NIP-65) silently resolves to no relays. Must be called before `Start`.
    pub fn set_routing(
        &mut self,
        router: Arc<dyn crate::substrate::OutboxRouter>,
        cache: Arc<dyn crate::substrate::MailboxCache>,
    ) {
        self.kernel.set_routing(router, cache);
    }

    /// Install a content parser on the wrapped kernel (wasm composition seam).
    pub fn set_content_parser(&mut self, parser: Arc<dyn crate::substrate::ContentParser>) {
        self.kernel.set_content_parser(parser);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl super::KernelReducer {
    #[allow(clippy::too_many_arguments)]
    pub fn project_raw_event_for_test(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: &str,
    ) {
        self.kernel
            .project_raw_event_for_test(id, pubkey, created_at, kind, tags, content);
    }

}
