//! NIP-29 group-scoped chat-presence read composition.

use std::sync::Arc;

use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use nmp_nip29::GroupId;
use nmp_ownership::DeclaredProjectionKey;
use nmp_read_session::{
    close_read, open_read, ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy,
    ReadSpec,
};

use crate::presence::ChatPresenceProjection;
use crate::wire::{
    encode_chat_presence_snapshot, CHAT_PRESENCE_FILE_IDENTIFIER, CHAT_PRESENCE_SCHEMA_ID,
    CHAT_PRESENCE_SCHEMA_VERSION,
};

const SCOPE_GLOBAL: u32 = 1;
const CHAT_PRESENCE_REPLAY_LIMIT: usize = 80;
const CHAT_PRESENCE_CONSUMER: &str = "chat-presence";

/// Snapshot key + singleton session key for chat read-state and typing presence.
pub const CHAT_PRESENCE_KEY: &str = "nmp.chat.presence";
const CHAT_PRESENCE_PROJECTION_TOKEN: DeclaredProjectionKey =
    DeclaredProjectionKey::framework(CHAT_PRESENCE_KEY, "projection.nmp.chat.presence");

/// Descriptor for a group-scoped chat-presence typed read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPresenceSession {
    group_id: GroupId,
    active_pubkey: String,
    message_kinds: Vec<u32>,
}

impl ChatPresenceSession {
    #[must_use]
    pub fn new(group_id: GroupId, active_pubkey: String, message_kinds: Vec<u32>) -> Self {
        Self {
            group_id,
            active_pubkey,
            message_kinds,
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

    #[must_use]
    pub fn message_kinds(&self) -> &[u32] {
        &self.message_kinds
    }
}

/// Runtime handle for one chat-presence read session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPresenceHandle(ReadHandle);

impl ChatPresenceHandle {
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.projection_key
    }
}

#[must_use]
pub fn open_chat_presence_session(
    host: &dyn ReadHost,
    descriptor: ChatPresenceSession,
) -> ChatPresenceHandle {
    let (handle, _) = open_chat_presence_session_with_reader(host, descriptor);
    handle
}

#[must_use]
pub fn open_chat_presence_session_with_reader(
    host: &dyn ReadHost,
    descriptor: ChatPresenceSession,
) -> (ChatPresenceHandle, Arc<ChatPresenceProjection>) {
    let ChatPresenceSession {
        group_id,
        active_pubkey,
        message_kinds,
    } = descriptor;
    let filter_json = chat_presence_filter_json(&group_id, &message_kinds);
    let relay_pin = Some(group_id.host_relay_url.clone());
    let projection = Arc::new(ChatPresenceProjection::new(
        group_id.host_relay_url.clone(),
        group_id.local_id.clone(),
        active_pubkey,
        message_kinds,
    ));
    let projection_reader = Arc::clone(&projection);

    let projection_for_output = Arc::clone(&projection);
    let output_encoder: ReadOutputEncoder = Box::new(move || {
        let snapshot = projection_for_output.snapshot();
        Some(TypedProjectionData {
            key: CHAT_PRESENCE_KEY.to_string(),
            schema_id: CHAT_PRESENCE_SCHEMA_ID.to_string(),
            schema_version: CHAT_PRESENCE_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CHAT_PRESENCE_FILE_IDENTIFIER).into_owned(),
            payload: encode_chat_presence_snapshot(&snapshot),
            ..Default::default()
        })
    });

    let _ = host.close_read_session_by_projection_key(CHAT_PRESENCE_KEY);
    let read_handle = open_read(
        host,
        ReadSpec {
            projection_key: CHAT_PRESENCE_PROJECTION_TOKEN.into(),
            demands: vec![ReadDemand {
                filter_json,
                consumer_id: CHAT_PRESENCE_CONSUMER.to_string(),
                scope: SCOPE_GLOBAL,
                relay_pin,
                is_indexer_discovery: false,
                replay_limit: CHAT_PRESENCE_REPLAY_LIMIT,
                replay: ReadReplayPolicy::Structural,
            }],
            observer: projection as Arc<dyn ObservedProjectionSink>,
            output_encoder,
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: false,
        },
    );
    (ChatPresenceHandle(read_handle), projection_reader)
}

#[must_use]
pub fn close_chat_presence_session(host: &dyn ReadHost, handle: ChatPresenceHandle) -> bool {
    close_read(host, &handle.0)
}

/// NIP-01 `REQ` filter for one group's chat-presence source messages.
///
/// `message_kinds` is caller-owned. Empty means all h-tagged group events,
/// matching the group-events read session's normalization.
#[must_use]
pub fn chat_presence_filter_json(group_id: &GroupId, message_kinds: &[u32]) -> String {
    let mut map = serde_json::Map::new();
    let mut kinds = message_kinds.to_vec();
    kinds.sort_unstable();
    kinds.dedup();
    if !kinds.is_empty() {
        map.insert("kinds".to_string(), serde_json::json!(kinds));
    }
    map.insert("#h".to_string(), serde_json::json!([group_id.local_id]));
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
#[path = "group_tests.rs"]
mod tests;
