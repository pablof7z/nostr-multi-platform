//! NIP-29 group-roster typed read-session composition.
//!
//! Split out of `group_feed.rs` (file-size cap). Provides the
//! `open_nip29_group_roster_session[_with_reader]` /
//! `close_nip29_group_roster_session` entry points; the shared open/teardown
//! plumbing lives in the sibling `feed` submodule.

use std::sync::Arc;

use nmp_nip29::group_id::group_roster_filter_json;
use nmp_nip29::{
    encode_group_roster_snapshot, GroupRosterProjection, GROUP_ROSTER_FILE_IDENTIFIER,
    GROUP_ROSTER_SCHEMA_ID, GROUP_ROSTER_SCHEMA_VERSION,
};

use crate::app_struct::NmpApp;

use super::{
    Nip29GroupRosterHandle, Nip29GroupRosterSession, GROUP_ROSTER_CONSUMER, GROUP_ROSTER_KEY,
    GROUP_ROSTER_PROJECTION_TOKEN, SCOPE_GLOBAL,
};

impl NmpApp {
    /// Open the NIP-29 member-roster typed read session for one group.
    /// Hydrating: a view opened after the group's 39001/39002/39003 snapshots
    /// were cached catches them up, then tails live. Pinned `Global` (the group
    /// host relay). Singleton: re-opening replaces the prior roster view.
    #[must_use]
    pub fn open_nip29_group_roster_session(
        &self,
        descriptor: Nip29GroupRosterSession,
    ) -> Nip29GroupRosterHandle {
        let (handle, _) = self.open_nip29_group_roster_session_with_reader(descriptor);
        handle
    }

    /// Open a group-roster typed read session and return the canonical
    /// projection reader.
    ///
    /// The returned [`GroupRosterProjection`] is the same `Arc` registered as
    /// the observed projection and used by the `"nmp.nip29.group_roster"` typed
    /// sidecar. Callers must not open a second roster observer; use this reader
    /// and keep the sidecar, relay-pinned interest, and hydration single-owned
    /// by this door.
    #[must_use]
    pub fn open_nip29_group_roster_session_with_reader(
        &self,
        descriptor: Nip29GroupRosterSession,
    ) -> (Nip29GroupRosterHandle, Arc<GroupRosterProjection>) {
        let Nip29GroupRosterSession { group_id } = descriptor;
        let relay_pin = Some(group_id.host_relay_url.clone());
        let filter_json = group_roster_filter_json(&group_id.local_id);
        let projection = Arc::new(GroupRosterProjection::new(
            group_id.host_relay_url.clone(),
            group_id.local_id.clone(),
        ));
        let projection_reader = Arc::clone(&projection);

        let projection_for_sidecar = Arc::clone(&projection);
        let register_sidecar = move |app: &NmpApp| {
            app.register_typed_snapshot_projection(GROUP_ROSTER_PROJECTION_TOKEN, move || {
                let snapshot = projection_for_sidecar.snapshot();
                Some(nmp_core::TypedProjectionData {
                    key: GROUP_ROSTER_KEY.to_string(),
                    schema_id: GROUP_ROSTER_SCHEMA_ID.to_string(),
                    schema_version: GROUP_ROSTER_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(GROUP_ROSTER_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_group_roster_snapshot(&snapshot),
                    ..Default::default()
                })
            });
        };

        let handle_id = self.open_group_feed(
            GROUP_ROSTER_KEY,
            GROUP_ROSTER_CONSUMER,
            SCOPE_GLOBAL,
            relay_pin,
            filter_json,
            projection as Arc<dyn nmp_core::ObservedProjectionSink>,
            register_sidecar,
        );
        (
            Nip29GroupRosterHandle {
                key: GROUP_ROSTER_KEY.to_string(),
                handle_id,
            },
            projection_reader,
        )
    }

    /// Close the group-roster typed read session represented by `handle`.
    /// Idempotent (D6).
    pub fn close_nip29_group_roster_session(&self, handle: Nip29GroupRosterHandle) {
        self.close_group_feed_handle(&handle.key, handle.handle_id);
    }
}
