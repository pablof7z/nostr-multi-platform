//! Browser-runtime NIP-29 group-events sessions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;
use nmp_nip29::{
    encode_group_events_snapshot, GroupEventsProjection, GroupEventsQuery, GroupId,
    GROUP_EVENTS_FILE_IDENTIFIER, GROUP_EVENTS_SCHEMA_ID, GROUP_EVENTS_SCHEMA_VERSION,
};
use nmp_planner::InterestShape;

use super::handle::BrowserRuntimeHandle;

const SCOPE_GLOBAL: u32 = 1;
const GROUP_EVENTS_KEY: &str = "nmp.nip29.group_events";
static NEXT_GROUP_EVENTS_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct BrowserGroupEventsSession {
    projection_key: String,
    handle_id: u64,
    observer_ids: Vec<ObservedProjectionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserGroupEventsSessionDescriptor {
    pub(crate) relay_url: String,
    pub(crate) group_id: String,
    pub(crate) kinds: Vec<u32>,
    pub(crate) session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserGroupEventsSessionHandle {
    session_id: String,
    projection_key: String,
    handle_id: u64,
}

impl BrowserGroupEventsSessionHandle {
    pub(crate) fn projection_key(&self) -> &str {
        &self.projection_key
    }
}

impl BrowserRuntimeHandle {
    /// Open a NIP-29 group-events read view for `group_id` constrained to the
    /// consumer-declared `kinds` (issue #2187). An empty `kinds` means all
    /// h-tagged group events; a chat view passes `[9, 11]`.
    ///
    /// Singleton: the snapshot key `nmp.nip29.group_events` is a static
    /// singleton, so opening first closes ANY existing group-events session.
    pub(crate) fn open_nip29_group_events_session(
        &mut self,
        descriptor: BrowserGroupEventsSessionDescriptor,
    ) -> Result<BrowserGroupEventsSessionHandle, String> {
        let relay = descriptor.relay_url.trim().to_string();
        let local_id = descriptor.group_id.trim().to_string();
        let session_id = descriptor.session_id;
        if relay.is_empty() {
            return Err("group events relay_url is required".to_string());
        }
        if local_id.is_empty() {
            return Err("group events group_id is required".to_string());
        }

        // Singleton semantics: opening closes any existing group-events session
        // (they all share the static snapshot key).
        self.close_all_group_events_sessions();
        let handle_id = NEXT_GROUP_EVENTS_HANDLE_ID.fetch_add(1, Ordering::Relaxed);

        let group = GroupId::new(relay.clone(), local_id.clone());
        group
            .require_routable()
            .map_err(|e| format!("invalid group events group_id: {e}"))?;
        // The SAME query builds the relay-interest filter and the projection's
        // accept predicate, so they can never diverge.
        let query = GroupEventsQuery::from_kinds(group, descriptor.kinds);
        let filter_json = query.filter_json();
        let mut shape = InterestShape::from_filter_json(&filter_json)
            .ok_or_else(|| "invalid group events filter".to_string())?;
        shape.relay_pin = Some(relay.clone());

        let projection = Arc::new(GroupEventsProjection::new(query));
        let projection_key = GROUP_EVENTS_KEY.to_string();
        self.register_group_events_sidecar(&projection_key, Arc::clone(&projection));

        let observer: Arc<dyn nmp_core::ObservedProjectionSink> = projection;
        let decl = ObservedProjection {
            observer,
            filter_json,
            consumer_id: group_events_consumer(&session_id, &relay, &local_id),
            scope: SCOPE_GLOBAL,
            relay_pin: Some(relay),
            replay_shapes: vec![shape],
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        };
        let id = self.observed_projection_registrar.open(decl);
        let observer_ids = if id.0 == 0 { Vec::new() } else { vec![id] };

        self.group_events_sessions.insert(
            session_id.clone(),
            BrowserGroupEventsSession {
                projection_key: projection_key.clone(),
                handle_id,
                observer_ids,
            },
        );
        Ok(BrowserGroupEventsSessionHandle {
            session_id,
            projection_key,
            handle_id,
        })
    }

    pub(crate) fn close_nip29_group_events_session(
        &mut self,
        handle: BrowserGroupEventsSessionHandle,
    ) {
        let should_close = self
            .group_events_sessions
            .get(&handle.session_id)
            .map(|session| session.handle_id == handle.handle_id)
            .unwrap_or(false);
        if should_close {
            self.close_nip29_group_events_session_by_id(&handle.session_id);
        }
    }

    pub(crate) fn close_nip29_group_events_session_by_id(&mut self, session_id: &str) {
        let Some(session) = self.group_events_sessions.remove(session_id) else {
            return;
        };
        self.teardown_group_events_session(session);
    }

    /// Close every live group-events session (singleton enforcement on open).
    fn close_all_group_events_sessions(&mut self) {
        let sessions: Vec<BrowserGroupEventsSession> =
            self.group_events_sessions.drain().map(|(_, s)| s).collect();
        for session in sessions {
            self.teardown_group_events_session(session);
        }
    }

    fn teardown_group_events_session(&mut self, session: BrowserGroupEventsSession) {
        for id in session.observer_ids {
            self.observed_projection_registrar.close(id);
        }
        self.runtime
            .reducer
            .remove_snapshot_projection(&session.projection_key);
    }

    fn register_group_events_sidecar(&mut self, key: &str, projection: Arc<GroupEventsProjection>) {
        let key_for_row = key.to_string();
        self.runtime
            .reducer
            .register_typed_snapshot_projection(key.to_string(), move || {
                let snapshot = projection.snapshot();
                Some(TypedProjectionData {
                    key: key_for_row.clone(),
                    schema_id: GROUP_EVENTS_SCHEMA_ID.to_string(),
                    schema_version: GROUP_EVENTS_SCHEMA_VERSION,
                    file_identifier: String::from_utf8_lossy(GROUP_EVENTS_FILE_IDENTIFIER)
                        .into_owned(),
                    payload: encode_group_events_snapshot(&snapshot),
                    ..Default::default()
                })
            });
    }
}

fn group_events_consumer(session_id: &str, relay: &str, group_id: &str) -> String {
    format!("group-events-{session_id}-{relay}-{group_id}")
}
