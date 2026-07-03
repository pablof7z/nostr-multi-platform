//! Concept-owned NIP-29 active read sessions.
//!
//! Runtime hosts implement `nmp_read_session::ReadHost` once. This crate owns
//! the NIP-29 descriptors, projection keys, filters, reducers, and typed
//! sidecar encoders, then drives the generic read engine through that host seam.

mod types;

use std::sync::Arc;

use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_ownership::DeclaredProjectionKey;
use nmp_read_session::{
    close_read, open_read, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy,
    ReadSpec,
};

use crate::group_id::{group_metadata_filter_json, group_roster_filter_json};
use crate::{
    encode_discovered_groups_snapshot, encode_group_events_snapshot, encode_group_roster_snapshot,
    encode_joined_groups_snapshot, DiscoveredGroupsProjection, GroupEventsProjection,
    GroupEventsQuery, GroupRosterProjection, JoinedGroupsProjection,
    DISCOVERED_GROUPS_FILE_IDENTIFIER, DISCOVERED_GROUPS_SCHEMA_ID,
    DISCOVERED_GROUPS_SCHEMA_VERSION, GROUP_EVENTS_FILE_IDENTIFIER, GROUP_EVENTS_SCHEMA_ID,
    GROUP_EVENTS_SCHEMA_VERSION, GROUP_ROSTER_FILE_IDENTIFIER, GROUP_ROSTER_SCHEMA_ID,
    GROUP_ROSTER_SCHEMA_VERSION, JOINED_GROUPS_FILE_IDENTIFIER, JOINED_GROUPS_SCHEMA_ID,
    JOINED_GROUPS_SCHEMA_VERSION,
};

pub use types::{
    Nip29GroupDiscoveryHandle, Nip29GroupDiscoverySession, Nip29GroupEventsHandle,
    Nip29GroupEventsSession, Nip29GroupRosterHandle, Nip29GroupRosterSession,
    Nip29JoinedGroupsHandle, Nip29JoinedGroupsSession,
};

const SCOPE_ACTIVE_ACCOUNT: u32 = 0;
const SCOPE_GLOBAL: u32 = 1;
const NIP29_GROUP_REPLAY_LIMIT: usize = 80;

pub const GROUP_EVENTS_KEY: &str = "nmp.nip29.group_events";
const GROUP_EVENTS_PROJECTION_TOKEN: DeclaredProjectionKey =
    DeclaredProjectionKey::framework(GROUP_EVENTS_KEY, "projection.nmp.nip29.group_events");
pub const DISCOVERED_GROUPS_KEY: &str = "nmp.nip29.discovered_groups";
const DISCOVERED_GROUPS_PROJECTION_TOKEN: DeclaredProjectionKey = DeclaredProjectionKey::framework(
    DISCOVERED_GROUPS_KEY,
    "projection.nmp.nip29.discovered_groups",
);
pub const JOINED_GROUPS_KEY: &str = "nmp.nip29.joined_groups";
const JOINED_GROUPS_PROJECTION_TOKEN: DeclaredProjectionKey =
    DeclaredProjectionKey::framework(JOINED_GROUPS_KEY, "projection.nmp.nip29.joined_groups");
pub const GROUP_ROSTER_KEY: &str = "nmp.nip29.group_roster";
const GROUP_ROSTER_PROJECTION_TOKEN: DeclaredProjectionKey =
    DeclaredProjectionKey::framework(GROUP_ROSTER_KEY, "projection.nmp.nip29.group_roster");

const GROUP_EVENTS_CONSUMER: &str = "nip29-group-events";
const DISCOVERED_GROUPS_CONSUMER: &str = "nip29-discovered-groups";
const JOINED_GROUPS_CONSUMER: &str = "nip29-joined-groups";
const GROUP_ROSTER_CONSUMER: &str = "nip29-group-roster";

#[must_use]
pub fn open_nip29_group_events_session(
    host: &dyn ReadHost,
    descriptor: Nip29GroupEventsSession,
) -> Nip29GroupEventsHandle {
    let (handle, _) = open_nip29_group_events_session_with_reader(host, descriptor);
    handle
}

#[must_use]
pub fn open_nip29_group_events_session_with_reader(
    host: &dyn ReadHost,
    descriptor: Nip29GroupEventsSession,
) -> (Nip29GroupEventsHandle, Arc<GroupEventsProjection>) {
    let Nip29GroupEventsSession { group_id, kinds } = descriptor;
    let relay_pin = Some(group_id.host_relay_url.clone());
    let query = GroupEventsQuery::from_kinds(group_id, kinds);
    let filter_json = query.filter_json();
    let projection = Arc::new(GroupEventsProjection::new(query));
    let projection_reader = Arc::clone(&projection);

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: GROUP_EVENTS_KEY.to_string(),
            schema_id: GROUP_EVENTS_SCHEMA_ID.to_string(),
            schema_version: GROUP_EVENTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(GROUP_EVENTS_FILE_IDENTIFIER).into_owned(),
            payload: encode_group_events_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let read_handle = open_group_read(
        host,
        GROUP_EVENTS_PROJECTION_TOKEN,
        GROUP_EVENTS_CONSUMER,
        SCOPE_GLOBAL,
        relay_pin,
        filter_json,
        projection as Arc<dyn ObservedProjectionSink>,
        output_encoder,
    );
    (Nip29GroupEventsHandle(read_handle), projection_reader)
}

#[must_use]
pub fn close_nip29_group_events_session(
    host: &dyn ReadHost,
    handle: Nip29GroupEventsHandle,
) -> bool {
    close_read(host, &handle.0)
}

/// Close the singleton NIP-29 group-events read by its concept-owned output key.
#[must_use]
pub fn close_nip29_group_events_read_by_key(host: &dyn ReadHost) -> bool {
    host.close_read_session_by_projection_key(GROUP_EVENTS_KEY)
}

#[must_use]
pub fn open_nip29_group_discovery_session(
    host: &dyn ReadHost,
    descriptor: Nip29GroupDiscoverySession,
) -> Nip29GroupDiscoveryHandle {
    let (handle, _) = open_nip29_group_discovery_session_with_reader(host, descriptor);
    handle
}

#[must_use]
pub fn open_nip29_group_discovery_session_with_reader(
    host: &dyn ReadHost,
    descriptor: Nip29GroupDiscoverySession,
) -> (Nip29GroupDiscoveryHandle, Arc<DiscoveredGroupsProjection>) {
    let Nip29GroupDiscoverySession { host_relay_url } = descriptor;
    let relay_pin = Some(host_relay_url.clone());
    let projection = Arc::new(DiscoveredGroupsProjection::new(host_relay_url));
    let projection_reader = Arc::clone(&projection);

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: DISCOVERED_GROUPS_KEY.to_string(),
            schema_id: DISCOVERED_GROUPS_SCHEMA_ID.to_string(),
            schema_version: DISCOVERED_GROUPS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(DISCOVERED_GROUPS_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_discovered_groups_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let read_handle = open_group_read(
        host,
        DISCOVERED_GROUPS_PROJECTION_TOKEN,
        DISCOVERED_GROUPS_CONSUMER,
        SCOPE_GLOBAL,
        relay_pin,
        group_metadata_filter_json(),
        projection as Arc<dyn ObservedProjectionSink>,
        output_encoder,
    );
    (Nip29GroupDiscoveryHandle(read_handle), projection_reader)
}

#[must_use]
pub fn close_nip29_group_discovery_session(
    host: &dyn ReadHost,
    handle: Nip29GroupDiscoveryHandle,
) -> bool {
    close_read(host, &handle.0)
}

/// Close the singleton NIP-29 group-discovery read by its concept-owned output key.
#[must_use]
pub fn close_nip29_group_discovery_read_by_key(host: &dyn ReadHost) -> bool {
    host.close_read_session_by_projection_key(DISCOVERED_GROUPS_KEY)
}

#[must_use]
pub fn open_nip29_joined_groups_session(
    host: &dyn ReadHost,
    descriptor: Nip29JoinedGroupsSession,
) -> Option<Nip29JoinedGroupsHandle> {
    open_nip29_joined_groups_session_with_reader(host, descriptor).map(|(handle, _)| handle)
}

#[must_use]
pub fn open_nip29_joined_groups_session_with_reader(
    host: &dyn ReadHost,
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

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: JOINED_GROUPS_KEY.to_string(),
            schema_id: JOINED_GROUPS_SCHEMA_ID.to_string(),
            schema_version: JOINED_GROUPS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(JOINED_GROUPS_FILE_IDENTIFIER).into_owned(),
            payload: encode_joined_groups_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let read_handle = open_group_read(
        host,
        JOINED_GROUPS_PROJECTION_TOKEN,
        JOINED_GROUPS_CONSUMER,
        SCOPE_ACTIVE_ACCOUNT,
        relay_pin,
        group_metadata_filter_json(),
        projection as Arc<dyn ObservedProjectionSink>,
        output_encoder,
    );
    Some((Nip29JoinedGroupsHandle(read_handle), projection_reader))
}

#[must_use]
pub fn close_nip29_joined_groups_session(
    host: &dyn ReadHost,
    handle: Nip29JoinedGroupsHandle,
) -> bool {
    close_read(host, &handle.0)
}

#[must_use]
pub fn open_nip29_group_roster_session(
    host: &dyn ReadHost,
    descriptor: Nip29GroupRosterSession,
) -> Nip29GroupRosterHandle {
    let (handle, _) = open_nip29_group_roster_session_with_reader(host, descriptor);
    handle
}

#[must_use]
pub fn open_nip29_group_roster_session_with_reader(
    host: &dyn ReadHost,
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

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: GROUP_ROSTER_KEY.to_string(),
            schema_id: GROUP_ROSTER_SCHEMA_ID.to_string(),
            schema_version: GROUP_ROSTER_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(GROUP_ROSTER_FILE_IDENTIFIER).into_owned(),
            payload: encode_group_roster_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let read_handle = open_group_read(
        host,
        GROUP_ROSTER_PROJECTION_TOKEN,
        GROUP_ROSTER_CONSUMER,
        SCOPE_GLOBAL,
        relay_pin,
        filter_json,
        projection as Arc<dyn ObservedProjectionSink>,
        output_encoder,
    );
    (Nip29GroupRosterHandle(read_handle), projection_reader)
}

#[must_use]
pub fn close_nip29_group_roster_session(
    host: &dyn ReadHost,
    handle: Nip29GroupRosterHandle,
) -> bool {
    close_read(host, &handle.0)
}

#[allow(clippy::too_many_arguments)]
fn open_group_read(
    host: &dyn ReadHost,
    key: DeclaredProjectionKey,
    consumer: &str,
    scope: u32,
    relay_pin: Option<String>,
    filter_json: String,
    observer: Arc<dyn ObservedProjectionSink>,
    output_encoder: ReadOutputEncoder,
) -> ReadHandle {
    let _ = host.close_read_session_by_projection_key(key.as_str());
    open_read(
        host,
        ReadSpec {
            projection_key: key.into(),
            demands: vec![ReadDemand {
                filter_json,
                consumer_id: consumer.to_string(),
                scope,
                relay_pin,
                replay_limit: NIP29_GROUP_REPLAY_LIMIT,
                replay: ReadReplayPolicy::Structural,
            }],
            observer,
            output_encoder,
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: false,
        },
    )
}
