//! NIP-29 per-open typed read-session composition + the reusable `NmpApp` Rust API.
//!
//! `nmp-native-runtime` is the composition root that owns the `NmpApp` actor
//! handle, so the host-driving NIP-29 read-view entrypoints live here — the same
//! composition role `search.rs` plays for NIP-50 search, and
//! `app_impl_feeds.rs` plays for declared feeds. The protocol semantics (the
//! projection types, the typed FlatBuffers sidecar codecs, the NmpApp-free
//! wire-filter builders) are owned by `nmp-nip29` (D0: `nmp-nip29` never names
//! `NmpApp`).
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
//! 2. open a relay-pinned observed projection internally via
//!    [`ObservedProjectionRegistrar::open_observed_projection`] — which registers
//!    the projection muted, replays the in-memory read-cache (ADR-0062
//!    `replay_read_cache_to_observer`, matched by the `#h` / kind shapes built
//!    from the same wire filter) to that observer, and THEN activates scoped
//!    live delivery for the declared interest;
//! 3. record the projection id keyed by the singleton projection key so the
//!    matching `close_*` can reverse the observed projection and remove the
//!    sidecar.
//!
//! The `StoreQuery::Tags` single-letter tag index landed (#2100), but the
//! durable-store leg of the `open_observed_projection` catch-up path is not yet
//! wired: `replay_read_cache_to_observer` scans only the in-memory `events`
//! cache and does not query the LMDB store for evicted events. Track in the
//! GitHub issue queue. The in-memory replay — the #2088 user-visible bug — is
//! fixed here and now.
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
//!
//! ## File organisation
//!
//! This module is split across submodules to stay within the 500-line cap:
//! - `mod.rs` — public session entry points for group-events, discovery, and
//!   joined-groups views, plus shared constants and the session-bookkeeping type.
//! - `roster` — entry points for the group-roster view.
//! - `feed` — shared `open_group_feed` / `close_group_feed` plumbing called by
//!   every view.
//! - `types` — descriptor and handle types for all four views.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use nmp_core::ObservedProjectionId;
use nmp_nip29::group_id::group_metadata_filter_json;
use nmp_nip29::{
    encode_discovered_groups_snapshot, encode_group_events_snapshot,
    encode_joined_groups_snapshot, DiscoveredGroupsProjection, GroupEventsProjection,
    GroupEventsQuery, JoinedGroupsProjection, DISCOVERED_GROUPS_FILE_IDENTIFIER,
    DISCOVERED_GROUPS_SCHEMA_ID, DISCOVERED_GROUPS_SCHEMA_VERSION, GROUP_EVENTS_FILE_IDENTIFIER,
    GROUP_EVENTS_SCHEMA_ID, GROUP_EVENTS_SCHEMA_VERSION, JOINED_GROUPS_FILE_IDENTIFIER,
    JOINED_GROUPS_SCHEMA_ID, JOINED_GROUPS_SCHEMA_VERSION,
};

use crate::app_struct::NmpApp;

mod feed;
mod reactions;
mod roster;
mod types;
pub use types::{
    Nip25GroupReactionsHandle, Nip25GroupReactionsSession, Nip29GroupDiscoveryHandle,
    Nip29GroupDiscoverySession, Nip29GroupEventsHandle, Nip29GroupEventsSession,
    Nip29GroupRosterHandle, Nip29GroupRosterSession, Nip29JoinedGroupsHandle,
    Nip29JoinedGroupsSession,
};

/// `0` = `ActiveAccount` scope (re-route on account switch) — the joined-groups
/// view, which is the active account's membership surface.
const SCOPE_ACTIVE_ACCOUNT: u32 = 0;
/// `1` = `Global` scope (account-agnostic) — the group-chat and discovery
/// views pin a concrete host relay and are not re-routed on account switch.
const SCOPE_GLOBAL: u32 = 1;

/// Snapshot key + singleton session key for the group-chat view.
pub const GROUP_EVENTS_KEY: &str = "nmp.nip29.group_events";
/// Snapshot key + singleton session key for the discovered-groups view.
pub const DISCOVERED_GROUPS_KEY: &str = "nmp.nip29.discovered_groups";
/// Snapshot key + singleton session key for the joined-groups view.
pub const JOINED_GROUPS_KEY: &str = "nmp.nip29.joined_groups";
/// Snapshot key + singleton session key for the group-roster view.
pub const GROUP_ROSTER_KEY: &str = "nmp.nip29.group_roster";
/// Snapshot key + singleton session key for the group-scoped reaction-aggregate
/// view (NIP-25 kind:7 folded by target id, scoped to one group's `h` tag).
pub const GROUP_REACTIONS_KEY: &str = "nmp.nip25.reactions";

/// Refcount-owner id for each (singleton) NIP-29 view's pinned interest.
const GROUP_EVENTS_CONSUMER: &str = "nip29-group-events";
const DISCOVERED_GROUPS_CONSUMER: &str = "nip29-discovered-groups";
const JOINED_GROUPS_CONSUMER: &str = "nip29-joined-groups";
const GROUP_ROSTER_CONSUMER: &str = "nip29-group-roster";
const GROUP_REACTIONS_CONSUMER: &str = "nip25-group-reactions";
static NEXT_GROUP_READ_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// Teardown recipe for one live NIP-29 read view (held in
/// `NmpApp::group_feed_sessions`, keyed by the view's projection key). Records
/// exactly what `open_*` installed so `close_*` reverses it.
pub(crate) struct GroupFeedSession {
    /// The snapshot-projection key (also the session key).
    projection_key: String,
    /// Unique id for the handle returned by this specific open.
    handle_id: u64,
    /// The observed-projection kernel observer id.
    observer_id: ObservedProjectionId,
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
    pub fn open_nip29_group_events_session(
        &self,
        descriptor: Nip29GroupEventsSession,
    ) -> Nip29GroupEventsHandle {
        let (handle, _) = self.open_nip29_group_events_session_with_reader(descriptor);
        handle
    }

    /// Open a group-events typed read session and return the canonical
    /// projection reader.
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
    pub fn open_nip29_group_events_session_with_reader(
        &self,
        descriptor: Nip29GroupEventsSession,
    ) -> (Nip29GroupEventsHandle, Arc<GroupEventsProjection>) {
        let Nip29GroupEventsSession { group_id, kinds } = descriptor;
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

        let handle_id = self.open_group_feed(
            GROUP_EVENTS_KEY,
            GROUP_EVENTS_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            filter_json,
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        (
            Nip29GroupEventsHandle {
                key: GROUP_EVENTS_KEY.to_string(),
                handle_id,
            },
            projection_reader,
        )
    }

    /// Close a group-events typed read session. Idempotent — closing an
    /// already-closed handle is a harmless no-op (D6).
    pub fn close_nip29_group_events_session(&self, handle: Nip29GroupEventsHandle) {
        self.close_group_feed_handle(&handle.key, handle.handle_id);
    }

    /// Open the NIP-29 group-discovery typed read session for one host relay. Hydrating:
    /// a view opened after the relay's kind:39000/39001/39002 catalog was
    /// cached catches it up (#2088), then tails live. Pinned `Global`.
    ///
    /// Returns a [`Nip29GroupDiscoveryHandle`] the caller passes to
    /// [`Self::close_nip29_group_discovery_session`] on teardown.
    /// Singleton: re-opening replaces the prior discovery view.
    #[must_use]
    pub fn open_nip29_group_discovery_session(
        &self,
        descriptor: Nip29GroupDiscoverySession,
    ) -> Nip29GroupDiscoveryHandle {
        let (handle, _) = self.open_nip29_group_discovery_session_with_reader(descriptor);
        handle
    }

    /// Open a group-discovery typed read session and return the canonical
    /// projection reader.
    ///
    /// This is the Rust-side app-composition API for hosts that need to fold
    /// discovered groups into an app-owned projection. The returned
    /// [`DiscoveredGroupsProjection`] is the same `Arc` registered as the
    /// observed projection and used by the `"nmp.nip29.discovered_groups"`
    /// typed sidecar. Callers must not open a second discovery projection just
    /// to compose over the catalog; use this reader and keep the sidecar,
    /// relay-pinned interest, and #2088 hydration single-owned by this door.
    #[must_use]
    pub fn open_nip29_group_discovery_session_with_reader(
        &self,
        descriptor: Nip29GroupDiscoverySession,
    ) -> (Nip29GroupDiscoveryHandle, Arc<DiscoveredGroupsProjection>) {
        let Nip29GroupDiscoverySession { host_relay_url } = descriptor;
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

        let handle_id = self.open_group_feed(
            DISCOVERED_GROUPS_KEY,
            DISCOVERED_GROUPS_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            group_metadata_filter_json(),
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );

        (
            Nip29GroupDiscoveryHandle {
                key: DISCOVERED_GROUPS_KEY.to_string(),
                handle_id,
            },
            projection_reader,
        )
    }

    /// Close the group-discovery typed read session represented by `handle`.
    /// Idempotent (D6).
    pub fn close_nip29_group_discovery_session(&self, handle: Nip29GroupDiscoveryHandle) {
        self.close_group_feed_handle(&handle.key, handle.handle_id);
    }

    /// Open the NIP-29 joined-groups typed read session for the active account.
    ///
    /// If `host_relay_url` is non-empty the projection is scoped to that host
    /// and the interest is pinned to it; otherwise the projection derives host
    /// identity from `KernelEvent.relay_provenance` and the interest is
    /// outbox-routed (no pin). Hydrating + `ActiveAccount`-scoped (re-routes on
    /// account switch). An empty `active_pubkey` is a no-op (D6). Singleton.
    ///
    /// Returns `None` when `active_pubkey` is empty and no session was opened.
    #[must_use]
    pub fn open_nip29_joined_groups_session(
        &self,
        descriptor: Nip29JoinedGroupsSession,
    ) -> Option<Nip29JoinedGroupsHandle> {
        self.open_nip29_joined_groups_session_with_reader(descriptor)
            .map(|(handle, _)| handle)
    }

    /// Open a joined-groups typed read session and return the canonical
    /// projection reader.
    ///
    /// This is the Rust-side app-composition API for hosts that need to fold
    /// active-account membership/admin truth into an app-owned projection. The
    /// returned [`JoinedGroupsProjection`] is the same `Arc` registered as the
    /// observed projection and used by the `"nmp.nip29.joined_groups"` typed
    /// sidecar. Returns `None` when `active_pubkey` is empty and no view was
    /// opened.
    #[must_use]
    pub fn open_nip29_joined_groups_session_with_reader(
        &self,
        descriptor: Nip29JoinedGroupsSession,
    ) -> Option<(Nip29JoinedGroupsHandle, Arc<JoinedGroupsProjection>)> {
        let Nip29JoinedGroupsSession {
            active_pubkey,
            host_relay_url,
        } = descriptor;
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

        let handle_id = self.open_group_feed(
            JOINED_GROUPS_KEY,
            JOINED_GROUPS_CONSUMER,
            SCOPE_ACTIVE_ACCOUNT,
            relay_pin,
            group_metadata_filter_json(),
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        Some((
            Nip29JoinedGroupsHandle {
                key: JOINED_GROUPS_KEY.to_string(),
                handle_id,
            },
            projection_reader,
        ))
    }

    /// Close the joined-groups typed read session represented by `handle`.
    /// Idempotent (D6).
    pub fn close_nip29_joined_groups_session(&self, handle: Nip29JoinedGroupsHandle) {
        self.close_group_feed_handle(&handle.key, handle.handle_id);
    }
}

#[cfg(test)]
#[path = "../group_feed_tests.rs"]
mod tests;
