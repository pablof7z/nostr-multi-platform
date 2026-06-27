//! Browser-runtime NIP-29 group timeline sessions.

use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip29::{
    encode_group_timeline_snapshot, GroupId, GroupTimelineProjection,
    GROUP_TIMELINE_FILE_IDENTIFIER, GROUP_TIMELINE_SCHEMA_ID, GROUP_TIMELINE_SCHEMA_VERSION,
};
use nmp_planner::InterestShape;

use super::handle::BrowserRuntimeHandle;

const SCOPE_GLOBAL: u32 = 1;
const GROUP_TIMELINE_KEY: &str = "nmp.nip29.group_timeline";

pub(crate) struct BrowserGroupTimelineSession {
    projection_key: String,
    observer_ids: Vec<ObservedProjectionId>,
}

impl BrowserRuntimeHandle {
    pub(crate) fn open_group_timeline(
        &mut self,
        relay_url: &str,
        group_id: &str,
        session_id: &str,
    ) -> Result<String, String> {
        let relay = relay_url.trim();
        let local_id = group_id.trim();
        if relay.is_empty() {
            return Err("group timeline relay_url is required".to_string());
        }
        if local_id.is_empty() {
            return Err("group timeline group_id is required".to_string());
        }

        self.close_group_timeline(session_id);

        let group = GroupId::new(relay.to_string(), local_id.to_string());
        group
            .require_routable()
            .map_err(|e| format!("invalid group timeline group_id: {e}"))?;
        let filter_json = group.chat_filter_json();
        let mut shape = InterestShape::from_filter_json(&filter_json)
            .ok_or_else(|| "invalid group timeline filter".to_string())?;
        shape.relay_pin = Some(relay.to_string());

        let projection = Arc::new(GroupTimelineProjection::new(group));
        let projection_key = GROUP_TIMELINE_KEY.to_string();
        self.register_group_timeline_sidecar(&projection_key, Arc::clone(&projection));

        let observer: Arc<dyn nmp_core::ObservedProjectionSink> = projection;
        let decl = ObservedProjection {
            observer,
            filter_json,
            consumer_id: group_timeline_consumer(session_id, relay, local_id),
            scope: SCOPE_GLOBAL,
            relay_pin: Some(relay.to_string()),
            replay_shapes: vec![shape],
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        };
        let id = self.observed_projection_registrar.open(decl);
        let observer_ids = if id.0 == 0 { Vec::new() } else { vec![id] };

        self.group_timeline_sessions.insert(
            session_id.to_string(),
            BrowserGroupTimelineSession {
                projection_key: projection_key.clone(),
                observer_ids,
            },
        );
        Ok(projection_key)
    }

    pub(crate) fn close_group_timeline(&mut self, session_id: &str) {
        let Some(session) = self.group_timeline_sessions.remove(session_id) else {
            return;
        };
        for id in session.observer_ids {
            self.observed_projection_registrar.close(id);
        }
        self.runtime
            .reducer
            .remove_snapshot_projection(&session.projection_key);
    }

    fn register_group_timeline_sidecar(
        &mut self,
        key: &str,
        projection: Arc<GroupTimelineProjection>,
    ) {
        let key_for_row = key.to_string();
        self.runtime
            .reducer
            .register_typed_snapshot_projection(key.to_string(), move || {
                let snapshot = projection.snapshot();
                Some(TypedProjectionData {
                    key: key_for_row.clone(),
                    schema_id: GROUP_TIMELINE_SCHEMA_ID.to_string(),
                    schema_version: GROUP_TIMELINE_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(GROUP_TIMELINE_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_group_timeline_snapshot(&snapshot),
                    ..Default::default()
                })
            });
    }
}

fn group_timeline_consumer(session_id: &str, relay: &str, group_id: &str) -> String {
    format!("group-timeline-{session_id}-{relay}-{group_id}")
}
