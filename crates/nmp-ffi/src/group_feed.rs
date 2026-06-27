//! NIP-29 per-open read-view composition + the reusable `NmpApp` Rust API.
//!
//! `nmp-ffi` is the composition root that owns the `NmpApp` actor handle, so the
//! host-driving NIP-29 read-view entrypoints live here — the same composition
//! role `search.rs` plays for NIP-50 search, and `app_impl_feeds.rs` plays for
//! declared feeds. The protocol semantics (the projection types, the typed
//! FlatBuffers sidecar codecs, the NmpApp-free wire-filter builders) are owned
//! by `nmp-nip29` (D0: `nmp-nip29` never names `NmpApp`).
//!
//! ## The #2088 fix
//!
//! The four per-open NIP-29 views (group chat, discovered groups, joined
//! groups, and the deleted raw group-events collector) used to register their
//! projection as a bare, already-active `ObservedProjectionSink` via
//! `nmp_nip29::register::wire_*`. A bare observer only sees the *global
//! fan-out* of LIVE ingest — so a view opened AFTER its events were already
//! accepted + cached hydrated live-only and silently dropped the cached tail
//! (#2088).
//!
//! Each surviving view now opens as a proper hydrating subscription, mirroring
//! `search.rs` step-for-step:
//!
//! 1. register the typed FlatBuffers sidecar reading the projection snapshot;
//! 2. open a relay-pinned observed projection via
//!    [`ObservedProjectionRegistrar::open_observed_projection`] — which registers
//!    the projection muted, replays the in-memory read-cache (ADR-0062
//!    `replay_read_cache_to_observer`, matched by the `#h` / kind shapes built
//!    from the same wire filter) to that observer, and THEN activates scoped
//!    live delivery for the declared interest;
//! 3. record the projection id keyed by the singleton projection key so the
//!    matching `close_*` can reverse the observed projection and remove the
//!    sidecar.
//!
//! The durable-store tail (events evicted from the bounded in-memory read-cache)
//! hydrates once the general single-letter (`#h`) `StoreQuery` index lands (a
//! separate effort generalizing `nmp-store`'s tag index); the in-memory replay
//! — the #2088 user-visible bug — is fixed here and now.
//!
//! ## Read-model parity
//!
//! The typed read-models (`GroupEventsProjection` → `NGEV`,
//! `DiscoveredGroupsProjection` → `NDGS`, `JoinedGroupsProjection` → `NJGS`)
//! and their snapshot keys / payloads are BYTE-IDENTICAL to the prior
//! `wire_*` registrations — only the ingest seam changed (bare-active →
//! muted-hydrating). Shells decode the same keys with the same schemas.
//!
//! ## D6
//!
//! Every entry point is fire-and-forget: an empty active pubkey, a poisoned
//! bookkeeping mutex, or a malformed filter degrades to a no-op open / partial
//! teardown rather than crossing the FFI as a panic.

use std::sync::Arc;

use nmp_core::substrate::{ObservedProjection, ObservedProjectionRegistrar};
use nmp_core::ObservedProjectionId;
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip29::group_id::{group_metadata_filter_json, GroupId};
use nmp_nip29::{
    encode_discovered_groups_snapshot, encode_group_events_snapshot,
    encode_joined_groups_snapshot, DiscoveredGroupsProjection, GroupEventsProjection,
    GroupEventsQuery, JoinedGroupsProjection, DISCOVERED_GROUPS_FILE_IDENTIFIER,
    DISCOVERED_GROUPS_SCHEMA_ID,
    DISCOVERED_GROUPS_SCHEMA_VERSION, GROUP_EVENTS_FILE_IDENTIFIER, GROUP_EVENTS_SCHEMA_ID,
    GROUP_EVENTS_SCHEMA_VERSION, JOINED_GROUPS_FILE_IDENTIFIER, JOINED_GROUPS_SCHEMA_ID,
    JOINED_GROUPS_SCHEMA_VERSION,
};

use crate::app_struct::NmpApp;

/// `0` = `ActiveAccount` scope (re-route on account switch) — the joined-groups
/// view, which is the active account's membership surface.
const SCOPE_ACTIVE_ACCOUNT: u32 = 0;
/// `1` = `Global` scope (account-agnostic) — the group-chat and discovery
/// views pin a concrete host relay and are not re-routed on account switch.
const SCOPE_GLOBAL: u32 = 1;

/// Snapshot key + singleton session key for the group-chat view.
const GROUP_EVENTS_KEY: &str = "nmp.nip29.group_events";
/// Snapshot key + singleton session key for the discovered-groups view.
const DISCOVERED_GROUPS_KEY: &str = "nmp.nip29.discovered_groups";
/// Snapshot key + singleton session key for the joined-groups view.
const JOINED_GROUPS_KEY: &str = "nmp.nip29.joined_groups";

/// Refcount-owner id for each (singleton) NIP-29 view's pinned interest.
const GROUP_EVENTS_CONSUMER: &str = "nip29-group-events";
const DISCOVERED_GROUPS_CONSUMER: &str = "nip29-discovered-groups";
const JOINED_GROUPS_CONSUMER: &str = "nip29-joined-groups";

/// Teardown recipe for one live NIP-29 read view (held in
/// `NmpApp::group_feed_sessions`, keyed by the view's projection key). Records
/// exactly what `open_*` installed so `close_*` reverses it.
pub(crate) struct GroupFeedSession {
    /// The snapshot-projection key (also the session key).
    projection_key: String,
    /// The observed-projection kernel observer id.
    observer_id: ObservedProjectionId,
}

/// Opaque handle for one host-driven NIP-29 read view, returned by the C-ABI
/// open wrappers that are fire-and-forget on the shell side (group discovery).
///
/// It carries only the owning app's address and the view's session key — both
/// `Send + Sync` plain data — so the matching `close` can reach the session the
/// open installed without the shell threading state through. The session itself
/// lives in `NmpApp::group_feed_sessions` (per-app, never a process-global
/// map), so there is no cross-app clobber.
///
/// SAFETY contract: the `NmpApp` used at open time MUST outlive this handle
/// (the same contract the prior `GroupDiscoveryHandle` carried).
pub struct GroupFeedHandle {
    /// Address of the owning `NmpApp` (stored as `usize` so the handle is
    /// `Send`/`Sync` without smuggling a raw pointer field through auto-trait
    /// analysis; reconstructed at close time).
    app_addr: usize,
    /// The view's session/projection key to close.
    key: String,
}

impl GroupFeedHandle {
    /// Tear down the view this handle owns: detach the pinned interest, revoke
    /// the observer, and remove the typed sidecar. Consumes the handle.
    ///
    /// # Safety
    /// The `NmpApp` passed at open time must still be alive (caller contract).
    pub unsafe fn close(self) {
        // SAFETY: `app_addr` is the address of a live `NmpApp` per the handle's
        // documented contract (the app outlives the handle).
        let app = unsafe { &*(self.app_addr as *const NmpApp) };
        app.close_group_feed(&self.key);
    }
}

impl NmpApp {
    /// Open the NIP-29 group-events read view for `group_id` constrained to the
    /// consumer-declared `kinds` (the reusable Rust API; the Chirp C-ABI thin
    /// shell `nmp_app_chirp_register_group_events` delegates here). Hydrating: a
    /// view opened after the group's events were already cached catches them up
    /// (#2088), then tails live.
    ///
    /// `kinds` is the consumer's kind selection (issue #2187): NIP-29 owns only
    /// the `["h", local_id]` routing; the caller chooses which kinds to read.
    /// An **empty** `kinds` means "all h-tagged group events". A chat view passes
    /// `[9, 11]`.
    ///
    /// Singleton: re-opening replaces the prior group-events view (idempotent at
    /// the registry level — the prior session is closed first, so navigating
    /// between groups never leaks the previous observer/interest).
    pub fn open_group_events(&self, group_id: GroupId, kinds: Vec<u32>) {
        let _ = self.open_group_events_with_reader(group_id, kinds);
    }

    /// Open the group-events view and return the canonical projection reader.
    ///
    /// This is the Rust-side app-composition API for hosts that need to read
    /// the selected group directly. The returned [`GroupEventsProjection`] is
    /// the same `Arc` registered as the observed projection and used by the
    /// `"nmp.nip29.group_events"` typed sidecar. Callers must not open a
    /// second group-events observer just to render the selected group; use this
    /// reader and keep the sidecar, relay-pinned interest, and #2088 hydration
    /// single-owned by this door.
    ///
    /// The same [`GroupEventsQuery`] builds BOTH the relay-interest `filter_json`
    /// and the projection's accept predicate, so the wire filter and the kind
    /// gate can never diverge.
    #[must_use]
    pub fn open_group_events_with_reader(
        &self,
        group_id: GroupId,
        kinds: Vec<u32>,
    ) -> Arc<GroupEventsProjection> {
        let relay_pin = Some(group_id.host_relay_url.clone());
        let query = GroupEventsQuery::from_kinds(group_id, kinds);
        let filter_json = query.filter_json();
        let projection = Arc::new(GroupEventsProjection::new(query));
        let projection_reader = Arc::clone(&projection);

        let projection_for_sidecar = Arc::clone(&projection);
        let register_sidecar = move |app: &NmpApp| {
            app.register_typed_snapshot_projection(GROUP_EVENTS_KEY, move || {
                let snapshot = projection_for_sidecar.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: GROUP_EVENTS_KEY.to_string(),
                    schema_id: GROUP_EVENTS_SCHEMA_ID.to_string(),
                    schema_version: GROUP_EVENTS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(GROUP_EVENTS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_group_events_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        };

        self.open_group_feed(
            GROUP_EVENTS_KEY,
            GROUP_EVENTS_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            filter_json,
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        projection_reader
    }

    /// Close the group-chat read view opened by [`Self::open_group_events`].
    /// Idempotent — closing an unopened view is a harmless no-op (D6).
    pub fn close_group_events(&self) {
        self.close_group_feed(GROUP_EVENTS_KEY);
    }

    /// Open the NIP-29 group-discovery read view for one host relay. Hydrating:
    /// a view opened after the relay's kind:39000/39001/39002 catalog was
    /// cached catches it up (#2088), then tails live. Pinned `Global`.
    ///
    /// Returns a [`GroupFeedHandle`] the caller passes to
    /// [`Self::close_group_feed`] (via the handle's `close`) on teardown.
    /// Singleton: re-opening replaces the prior discovery view.
    #[must_use]
    pub fn open_group_discovery(&self, host_relay_url: String) -> GroupFeedHandle {
        let (handle, _) = self.open_group_discovery_with_reader(host_relay_url);
        handle
    }

    /// Open group discovery and return the canonical projection reader.
    ///
    /// This is the Rust-side app-composition API for hosts that need to fold
    /// discovered groups into an app-owned projection. The returned
    /// [`DiscoveredGroupsProjection`] is the same `Arc` registered as the
    /// observed projection and used by the `"nmp.nip29.discovered_groups"`
    /// typed sidecar. Callers must not open a second discovery projection just
    /// to compose over the catalog; use this reader and keep the sidecar,
    /// relay-pinned interest, and #2088 hydration single-owned by this door.
    #[must_use]
    pub fn open_group_discovery_with_reader(
        &self,
        host_relay_url: String,
    ) -> (GroupFeedHandle, Arc<DiscoveredGroupsProjection>) {
        let relay_pin = Some(host_relay_url.clone());
        let projection = Arc::new(DiscoveredGroupsProjection::new(host_relay_url));
        let projection_reader = Arc::clone(&projection);

        let projection_for_sidecar = Arc::clone(&projection);
        let register_sidecar = move |app: &NmpApp| {
            app.register_typed_snapshot_projection(DISCOVERED_GROUPS_KEY, move || {
                let snapshot = projection_for_sidecar.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: DISCOVERED_GROUPS_KEY.to_string(),
                    schema_id: DISCOVERED_GROUPS_SCHEMA_ID.to_string(),
                    schema_version: DISCOVERED_GROUPS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(DISCOVERED_GROUPS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_discovered_groups_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        };

        self.open_group_feed(
            DISCOVERED_GROUPS_KEY,
            DISCOVERED_GROUPS_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            group_metadata_filter_json(),
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );

        (
            GroupFeedHandle {
                app_addr: (self as *const NmpApp) as usize,
                key: DISCOVERED_GROUPS_KEY.to_string(),
            },
            projection_reader,
        )
    }

    /// Close the group-discovery read view opened by
    /// [`Self::open_group_discovery`]. Idempotent (D6).
    pub fn close_group_discovery(&self) {
        self.close_group_feed(DISCOVERED_GROUPS_KEY);
    }

    /// Open the NIP-29 joined-groups read view for the active account.
    ///
    /// If `host_relay_url` is non-empty the projection is scoped to that host
    /// and the interest is pinned to it; otherwise the projection derives host
    /// identity from `KernelEvent.relay_provenance` and the interest is
    /// outbox-routed (no pin). Hydrating + `ActiveAccount`-scoped (re-routes on
    /// account switch). An empty `active_pubkey` is a no-op (D6). Singleton.
    pub fn open_joined_groups(&self, active_pubkey: String, host_relay_url: String) {
        let _ = self.open_joined_groups_with_reader(active_pubkey, host_relay_url);
    }

    /// Open joined groups and return the canonical projection reader.
    ///
    /// This is the Rust-side app-composition API for hosts that need to fold
    /// active-account membership/admin truth into an app-owned projection. The
    /// returned [`JoinedGroupsProjection`] is the same `Arc` registered as the
    /// observed projection and used by the `"nmp.nip29.joined_groups"` typed
    /// sidecar. Returns `None` when `active_pubkey` is empty and no view was
    /// opened.
    #[must_use]
    pub fn open_joined_groups_with_reader(
        &self,
        active_pubkey: String,
        host_relay_url: String,
    ) -> Option<Arc<JoinedGroupsProjection>> {
        if active_pubkey.is_empty() {
            return None;
        }
        let (projection, relay_pin) = if host_relay_url.is_empty() {
            (Arc::new(JoinedGroupsProjection::new(active_pubkey)), None)
        } else {
            (
                Arc::new(JoinedGroupsProjection::new_for_host(
                    active_pubkey,
                    host_relay_url.clone(),
                )),
                Some(host_relay_url),
            )
        };
        let projection_reader = Arc::clone(&projection);

        let projection_for_sidecar = Arc::clone(&projection);
        let register_sidecar = move |app: &NmpApp| {
            app.register_typed_snapshot_projection(JOINED_GROUPS_KEY, move || {
                let snapshot = projection_for_sidecar.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: JOINED_GROUPS_KEY.to_string(),
                    schema_id: JOINED_GROUPS_SCHEMA_ID.to_string(),
                    schema_version: JOINED_GROUPS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(JOINED_GROUPS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_joined_groups_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        };

        self.open_group_feed(
            JOINED_GROUPS_KEY,
            JOINED_GROUPS_CONSUMER,
            SCOPE_ACTIVE_ACCOUNT,
            relay_pin,
            group_metadata_filter_json(),
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        Some(projection_reader)
    }

    /// Close the joined-groups read view opened by [`Self::open_joined_groups`].
    /// Idempotent (D6).
    pub fn close_joined_groups(&self) {
        self.close_group_feed(JOINED_GROUPS_KEY);
    }

    /// Shared open path for the three hydrating NIP-29 read views.
    ///
    /// Idempotently tears down any prior session under `key` first (singleton
    /// semantics), registers the typed sidecar, registers the projection MUTED,
    /// then opens the relay-pinned observed interest with read-cache replay
    /// shapes derived from the same wire filter (so the in-memory cache is
    /// hydrated to the muted observer before it is activated — the #2088 fix).
    #[allow(clippy::too_many_arguments)]
    fn open_group_feed(
        &self,
        key: &str,
        consumer: &str,
        scope: u32,
        relay_pin: Option<String>,
        filter_json: String,
        observer: Arc<dyn nmp_core::ObservedProjectionSink>,
        register_sidecar: impl FnOnce(&NmpApp),
    ) {
        // Singleton: drop any prior session under this key first. Teardown must
        // run BEFORE the replacement registers — both sessions share the same
        // projection key, so a late key-based teardown would remove the new
        // view's sidecar.
        self.close_group_feed(key);

        register_sidecar(self);

        // The in-memory read-cache replay (ADR-0062) matches cached events by
        // the SAME wire shape the live filter uses — `matches_event_with_id`
        // honours the `#h` generic-tag + kind dimensions. A malformed filter
        // yields no shapes; `open_observed_projection` validates the filter and
        // no-ops the interest open while returning the observer id.
        let replay_shapes: Vec<nmp_planner::InterestShape> =
            nmp_planner::InterestShape::from_filter_json(&filter_json)
                .map(|mut shape| {
                    shape.relay_pin = relay_pin.clone();
                    shape
                })
                .into_iter()
                .collect();

        let observer_id = self.open_observed_projection(ObservedProjection {
            observer,
            filter_json,
            consumer_id: consumer.to_string(),
            scope,
            relay_pin,
            replay_shapes,
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        });
        if observer_id.0 == 0 {
            self.remove_snapshot_projection(key);
            return;
        }

        let Ok(mut sessions) = self.group_feed_sessions.lock() else {
            self.close_observed_projection(observer_id);
            self.remove_snapshot_projection(key);
            return;
        };
        sessions.insert(
            key.to_string(),
            GroupFeedSession {
                projection_key: key.to_string(),
                observer_id,
            },
        );
    }

    /// Tear down the NIP-29 read view registered under `key`: detach the pinned
    /// interest, revoke the observer, and remove the typed sidecar. Idempotent —
    /// closing an unknown view is a harmless no-op (D6).
    pub(crate) fn close_group_feed(&self, key: &str) {
        let session = self
            .group_feed_sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(key));
        let Some(session) = session else {
            return;
        };
        self.close_observed_projection(session.observer_id);
        self.remove_snapshot_projection(&session.projection_key);
    }
}
