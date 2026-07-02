//! Group-scoped threading-graph typed read-session composition (issue #2719).
//!
//! Provides the `open_nip29_group_threading_session[_with_reader]` /
//! `close_nip29_group_threading_session` entry points; the shared open/teardown
//! plumbing lives in the sibling `feed` submodule.
//!
//! ## Composition boundary
//!
//! `nmp-threading` owns kind-blind NIP-10 `e`-tag reply/root grammar (the
//! [`ThreadingProjection`](nmp_threading::ThreadingProjection) fold) and
//! NIP-29 owns the `["h", local_id]` group routing; neither crate names the
//! other (`crates/nmp-nip29/src/projection/group_events.rs`'s doc comment
//! points here). The *group-scoped threading view* is composed HERE, at the
//! app layer, by feeding the kind-blind threading projection the SAME
//! relay-pinned `#h` + kind interest a group-events view would use — so its
//! edges always cover the exact event set the caller already renders.

use std::sync::Arc;

use nmp_nip29::GroupEventsQuery;
use nmp_threading::{
    encode_threading_snapshot, EtagThreadResolver, ModulePolicy, ThreadingProjection,
    THREADING_GRAPH_FILE_IDENTIFIER, THREADING_GRAPH_SCHEMA_ID, THREADING_GRAPH_SCHEMA_VERSION,
};

use crate::app_struct::NmpApp;

use super::{
    Nip29GroupThreadingHandle, Nip29GroupThreadingSession, GROUP_THREADING_CONSUMER,
    GROUP_THREADING_KEY, GROUP_THREADING_PROJECTION_TOKEN, SCOPE_GLOBAL,
};

impl NmpApp {
    /// Open the group-scoped threading-graph typed read session for one group.
    /// Hydrating: a view opened after the group's events were already cached
    /// catches them up (#2088), then tails live. Pinned `Global` (the group
    /// host relay). Singleton: re-opening replaces the prior view.
    #[must_use]
    pub fn open_nip29_group_threading_session(
        &self,
        descriptor: Nip29GroupThreadingSession,
    ) -> Nip29GroupThreadingHandle {
        let (handle, _) = self.open_nip29_group_threading_session_with_reader(descriptor);
        handle
    }

    /// Open a group-scoped threading-graph typed read session and return the
    /// canonical projection reader.
    ///
    /// The returned [`ThreadingProjection`] is the same `Arc` registered as the
    /// observed projection and used by the `"nmp.threading.graph"` typed
    /// sidecar. Callers must not open a second threading observer for the same
    /// group; use this reader and keep the sidecar, relay-pinned interest, and
    /// hydration single-owned by this door.
    ///
    /// The same [`GroupEventsQuery`] a group-events view would build for
    /// `descriptor`'s group/kinds selects the relay-interest `filter_json`
    /// here, so the threading fold always observes exactly the events the
    /// paired group-events view renders.
    #[must_use]
    pub fn open_nip29_group_threading_session_with_reader(
        &self,
        descriptor: Nip29GroupThreadingSession,
    ) -> (
        Nip29GroupThreadingHandle,
        Arc<ThreadingProjection<EtagThreadResolver>>,
    ) {
        let Nip29GroupThreadingSession { group_id, kinds } = descriptor;
        let relay_pin = Some(group_id.host_relay_url.clone());
        let filter_json = GroupEventsQuery::from_kinds(group_id, kinds).filter_json();
        let projection = Arc::new(ThreadingProjection::etag(ModulePolicy::default()));
        let projection_reader = Arc::clone(&projection);

        let projection_for_sidecar = Arc::clone(&projection);
        let register_sidecar = move |app: &NmpApp| {
            app.register_typed_snapshot_projection(GROUP_THREADING_PROJECTION_TOKEN, move || {
                let snapshot = projection_for_sidecar.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: GROUP_THREADING_KEY.to_string(),
                    schema_id: THREADING_GRAPH_SCHEMA_ID.to_string(),
                    schema_version: THREADING_GRAPH_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(THREADING_GRAPH_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_threading_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        };

        let handle_id = self.open_group_feed(
            GROUP_THREADING_KEY,
            GROUP_THREADING_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            filter_json,
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        (
            Nip29GroupThreadingHandle {
                key: GROUP_THREADING_KEY.to_string(),
                handle_id,
            },
            projection_reader,
        )
    }

    /// Close the group-threading typed read session represented by `handle`.
    /// Idempotent (D6).
    pub fn close_nip29_group_threading_session(&self, handle: Nip29GroupThreadingHandle) {
        self.close_group_feed_handle(&handle.key, handle.handle_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip29::group_id::GroupId;

    #[test]
    fn filter_reuses_group_events_query_kinds_and_h_tag() {
        let group = GroupId::new("wss://groups.example.com", "room-a");
        let filter_json = GroupEventsQuery::from_kinds(group, vec![9, 11]).filter_json();
        let v: serde_json::Value = serde_json::from_str(&filter_json).unwrap();
        assert_eq!(v["kinds"], serde_json::json!([9, 11]));
        assert_eq!(v["#h"], serde_json::json!(["room-a"]));
        assert!(nmp_planner::InterestShape::from_filter_json(&filter_json).is_some());
    }
}
