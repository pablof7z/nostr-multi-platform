//! Browser-runtime NIP-29 public group-discovery sessions.

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

pub(crate) struct BrowserGroupDiscoverySession {
    projection_key: String,
    observer_ids: Vec<ObservedProjectionId>,
}

impl BrowserRuntimeHandle {
    pub(crate) fn open_group_discovery(
        &mut self,
        relay_url: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let relay = relay_url.trim();
        if relay.is_empty() {
            return Err("group discovery relay_url is required".to_string());
        }

        self.close_group_discovery(session_id);

        let projection = Arc::new(DiscoveredGroupsProjection::new(relay.to_string()));
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
            relay_pin: Some(relay.to_string()),
            ..Default::default()
        };

        let observer: Arc<dyn nmp_core::ObservedProjectionSink> = projection;
        let decl = ObservedProjection {
            observer,
            filter_json: group_metadata_filter_json(),
            consumer_id: group_discovery_consumer(session_id, relay),
            scope: SCOPE_GLOBAL,
            relay_pin: Some(relay.to_string()),
            replay_shapes: vec![shape],
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        };
        let id = self.observed_projection_registrar.open(decl);
        let observer_ids = if id.0 == 0 { Vec::new() } else { vec![id] };

        self.group_discovery_sessions.insert(
            session_id.to_string(),
            BrowserGroupDiscoverySession {
                projection_key: projection_key.clone(),
                observer_ids,
            },
        );
        Ok(projection_key)
    }

    pub(crate) fn close_group_discovery(&mut self, session_id: &str) {
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
