//! Group-scoped reaction aggregate read composition.
//!
//! NIP-25 owns kind:7 reaction semantics and the aggregate fold. NIP-29 owns
//! the group `h` envelope and host relay pin. This module composes those two
//! owners into a read-session doorway without putting the doorway on a runtime
//! type such as `NmpApp`.

use std::sync::Arc;

use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_nip25::{
    encode_reaction_aggregate_snapshot, ReactionAggregateProjection, KIND_REACTION,
    KIND_REACTION_DELETE, REACTION_AGGREGATE_FILE_IDENTIFIER, REACTION_AGGREGATE_SCHEMA_ID,
    REACTION_AGGREGATE_SCHEMA_VERSION,
};
use nmp_nip29::GroupId;
use nmp_ownership::DeclaredProjectionKey;
use nmp_read_session::{
    close_read, open_read, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy,
    ReadSpec,
};

const SCOPE_GLOBAL: u32 = 1;
const GROUP_REACTIONS_REPLAY_LIMIT: usize = 80;
const GROUP_REACTIONS_CONSUMER: &str = "nip25-group-reactions";

/// Snapshot key + singleton session key for the group-scoped reaction aggregate.
pub const GROUP_REACTIONS_KEY: &str = "nmp.nip25.reactions";
const GROUP_REACTIONS_PROJECTION_TOKEN: DeclaredProjectionKey =
    DeclaredProjectionKey::framework(GROUP_REACTIONS_KEY, "projection.nmp.nip25.reactions");

/// Descriptor for a group-scoped NIP-25 reaction-aggregate typed read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip25GroupReactionsSession {
    group_id: GroupId,
    active_pubkey: String,
}

impl Nip25GroupReactionsSession {
    #[must_use]
    pub fn new(group_id: GroupId, active_pubkey: String) -> Self {
        Self {
            group_id,
            active_pubkey,
        }
    }

    #[must_use]
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    #[must_use]
    pub fn active_pubkey(&self) -> &str {
        &self.active_pubkey
    }
}

/// Runtime handle for one group-scoped NIP-25 reaction-aggregate read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip25GroupReactionsHandle(ReadHandle);

impl Nip25GroupReactionsHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}

/// Open the group-scoped NIP-25 reaction-aggregate typed read session.
#[must_use]
pub fn open_nip25_group_reactions_session(
    host: &dyn ReadHost,
    descriptor: Nip25GroupReactionsSession,
) -> Nip25GroupReactionsHandle {
    let (handle, _) = open_nip25_group_reactions_session_with_reader(host, descriptor);
    handle
}

/// Open the group-scoped NIP-25 reaction-aggregate typed read session and
/// return the aggregate reader used by the typed sidecar.
#[must_use]
pub fn open_nip25_group_reactions_session_with_reader(
    host: &dyn ReadHost,
    descriptor: Nip25GroupReactionsSession,
) -> (Nip25GroupReactionsHandle, Arc<ReactionAggregateProjection>) {
    let Nip25GroupReactionsSession {
        group_id,
        active_pubkey,
    } = descriptor;
    let relay_pin = Some(group_id.host_relay_url.clone());
    let filter_json = group_reactions_filter_json(&group_id);
    let projection = Arc::new(ReactionAggregateProjection::new(Some(active_pubkey)));
    let projection_reader = Arc::clone(&projection);

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: GROUP_REACTIONS_KEY.to_string(),
            schema_id: REACTION_AGGREGATE_SCHEMA_ID.to_string(),
            schema_version: REACTION_AGGREGATE_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(REACTION_AGGREGATE_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_reaction_aggregate_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let _ = host.close_read_session_by_projection_key(GROUP_REACTIONS_KEY);
    let read_handle = open_read(
        host,
        ReadSpec {
            projection_key: GROUP_REACTIONS_PROJECTION_TOKEN.into(),
            demands: vec![ReadDemand {
                filter_json,
                consumer_id: GROUP_REACTIONS_CONSUMER.to_string(),
                scope: SCOPE_GLOBAL,
                relay_pin,
                replay_limit: GROUP_REACTIONS_REPLAY_LIMIT,
                replay: ReadReplayPolicy::Structural,
            }],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: false,
        },
    );
    (Nip25GroupReactionsHandle(read_handle), projection_reader)
}

/// Close the group-reactions typed read session represented by `handle`.
#[must_use]
pub fn close_nip25_group_reactions_session(
    host: &dyn ReadHost,
    handle: Nip25GroupReactionsHandle,
) -> bool {
    close_read(host, &handle.0)
}

/// NIP-01 `REQ` filter for one group's reactions:
/// `{"kinds":[5,7],"#h":["<id>"]}`.
///
/// This composes the NIP-25 reaction kind (7), NIP-09 deletion kind (5), and
/// NIP-29 `h` routing tag. The host relay pin is carried separately through
/// the read-session demand; it is not serialized into the wire filter.
#[must_use]
pub fn group_reactions_filter_json(group_id: &GroupId) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "kinds".to_string(),
        serde_json::json!([KIND_REACTION_DELETE, KIND_REACTION]),
    );
    map.insert("#h".to_string(), serde_json::json!([group_id.local_id]));
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
#[path = "group_tests.rs"]
mod tests;
