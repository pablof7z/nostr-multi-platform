//! Browser-runtime NIP-29 public group-discovery sessions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip29::group_id::group_metadata_filter_json;
use nmp_nip29::{
    encode_discovered_groups_snapshot, DiscoveredGroupsProjection,
    DISCOVERED_GROUPS_FILE_IDENTIFIER, DISCOVERED_GROUPS_SCHEMA_ID,
    DISCOVERED_GROUPS_SCHEMA_VERSION,
};
use nmp_planner::InterestShape;

use super::handle::BrowserRuntimeHandle;

const SCOPE_GLOBAL: u32 = 1;
const DISCOVERED_GROUPS_KEY: &str = "nmp.nip29.discovered_groups";
static NEXT_GROUP_DISCOVERY_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct BrowserGroupDiscoverySession {
    projection_key: String,
    handle_id: u64,
    observer_ids: Vec<ObservedProjectionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserGroupDiscoverySessionDescriptor {
    pub(crate) relay_url: String,
    pub(crate) session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserGroupDiscoverySessionHandle {
    session_id: String,
    projection_key: String,
    handle_id: u64,
}

impl BrowserGroupDiscoverySessionHandle {
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.projection_key
    }
}

impl BrowserRuntimeHandle {
    pub(crate) fn open_nip29_group_discovery_session(
        &mut self,
        descriptor: BrowserGroupDiscoverySessionDescriptor,
    ) -> Result<BrowserGroupDiscoverySessionHandle, String> {
        let relay = descriptor.relay_url.trim().to_string();
        if relay.is_empty() {
            return Err("group discovery relay_url is required".to_string());
        }
        let session_id = descriptor.session_id;

        self.close_nip29_group_discovery_session_by_id(&session_id);
        let handle_id = NEXT_GROUP_DISCOVERY_HANDLE_ID.fetch_add(1, Ordering::Relaxed);

        let projection = Arc::new(DiscoveredGroupsProjection::new(relay.clone()));
        let projection_key = DISCOVERED_GROUPS_KEY.to_string();
        self.register_group_discovery_sidecar(&projection_key, Arc::clone(&projection));

        let shape = InterestShape {
            kinds: [
                nmp_nip29::kinds::KIND_GROUP_METADATA,
                nmp_nip29::kinds::KIND_GROUP_ADMINS,
                nmp_nip29::kinds::KIND_GROUP_MEMBERS,
            ]
            .into_iter()
            .collect(),
            relay_pin: Some(relay.clone()),
            ..Default::default()
        };

        let observer: Arc<dyn nmp_core::ObservedProjectionSink> = projection;
        let decl = ObservedProjection {
            observer,
            filter_json: group_metadata_filter_json(),
            consumer_id: group_discovery_consumer(&session_id, &relay),
            scope: SCOPE_GLOBAL,
            relay_pin: Some(relay),
            replay_shapes: vec![shape],
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        };
        let id = self.observed_projection_registrar.open(decl);
        let observer_ids = if id.0 == 0 { Vec::new() } else { vec![id] };

        self.group_discovery_sessions.insert(
            session_id.to_string(),
            BrowserGroupDiscoverySession {
                projection_key: projection_key.clone(),
                handle_id,
                observer_ids,
            },
        );
        Ok(BrowserGroupDiscoverySessionHandle {
            session_id,
            projection_key,
            handle_id,
        })
    }

    pub fn close_nip29_group_discovery_session(
        &mut self,
        handle: BrowserGroupDiscoverySessionHandle,
    ) {
        let should_close = self
            .group_discovery_sessions
            .get(&handle.session_id)
            .map(|session| session.handle_id == handle.handle_id)
            .unwrap_or(false);
        if should_close {
            self.close_nip29_group_discovery_session_by_id(&handle.session_id);
        }
    }

    pub(crate) fn close_nip29_group_discovery_session_by_id(&mut self, session_id: &str) {
        let Some(session) = self.group_discovery_sessions.remove(session_id) else {
            return;
        };
        for id in session.observer_ids {
            self.observed_projection_registrar.close(id);
        }
        self.runtime
            .reducer
            .remove_snapshot_projection(&session.projection_key);
    }

    fn register_group_discovery_sidecar(
        &mut self,
        key: &str,
        projection: Arc<DiscoveredGroupsProjection>,
    ) {
        let key_for_row = key.to_string();
        self.runtime
            .reducer
            .register_typed_snapshot_projection(key.to_string(), move || {
                let snapshot = projection.snapshot();
                Some(TypedProjectionData {
                    key: key_for_row.clone(),
                    schema_id: DISCOVERED_GROUPS_SCHEMA_ID.to_string(),
                    schema_version: DISCOVERED_GROUPS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(DISCOVERED_GROUPS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_discovered_groups_snapshot(&snapshot),
                    ..Default::default()
                })
            });
    }
}

fn group_discovery_consumer(session_id: &str, relay: &str) -> String {
    format!("group-discovery-{session_id}-{relay}")
}
