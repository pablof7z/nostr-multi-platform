//! Browser-runtime notification sessions.

use std::sync::Arc;

use nmp_core::substrate::ObservedProjection;
use nmp_core::{ObservedProjectionId, TypedProjectionData};
use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;

mod projection;
mod wire;

use projection::{
    notifications_interest_shape, NotificationsProjection, NOTIFICATIONS_KEY,
    NOTIFICATIONS_SCHEMA_ID, NOTIFICATIONS_SCHEMA_VERSION,
};
use wire::{encode_notifications_snapshot, notifications_file_identifier};

use super::handle::BrowserRuntimeHandle;

const SCOPE_GLOBAL: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNotificationsSessionDescriptor {
    pub(crate) account_pubkey: String,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNotificationsSessionHandle {
    key: String,
}

impl BrowserNotificationsSessionHandle {
    #[must_use]
    pub(crate) fn for_key(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

pub(crate) struct BrowserNotificationsSession {
    projection_key: String,
    projection: Arc<NotificationsProjection>,
    observer_ids: Vec<ObservedProjectionId>,
}

impl BrowserRuntimeHandle {
    pub(crate) fn open_notifications_session(
        &mut self,
        descriptor: BrowserNotificationsSessionDescriptor,
    ) -> Result<BrowserNotificationsSessionHandle, String> {
        let pubkey = descriptor.account_pubkey.trim();
        if !is_hex64(pubkey) {
            return Err("notifications require a 64-hex account pubkey".to_string());
        }

        self.close_notifications_key(&descriptor.key);

        let projection = Arc::new(NotificationsProjection::new(pubkey.to_string()));
        let projection_key = notifications_key(&descriptor.key);
        self.register_notifications_sidecar(&projection_key, Arc::clone(&projection));

        let shape = notifications_interest_shape(pubkey);
        let filter_json = nmp_core::subs::filter_json_for(&shape);
        let observer: Arc<dyn nmp_core::ObservedProjectionSink> = projection.clone();
        let decl = ObservedProjection {
            observer,
            filter_json,
            consumer_id: notifications_consumer(&descriptor.key, pubkey),
            scope: SCOPE_GLOBAL,
            relay_pin: None,
            replay_shapes: vec![shape],
            replay_limit: DEFAULT_FEED_WINDOW_LIMIT,
        };
        let id = self.observed_projection_registrar.open(decl);
        let observer_ids = if id.0 == 0 { Vec::new() } else { vec![id] };

        self.notifications_sessions.insert(
            descriptor.key.clone(),
            BrowserNotificationsSession {
                projection_key: projection_key.clone(),
                projection,
                observer_ids,
            },
        );
        Ok(BrowserNotificationsSessionHandle {
            key: descriptor.key,
        })
    }

    pub(crate) fn mark_notifications_session_read(
        &mut self,
        handle: &BrowserNotificationsSessionHandle,
        event_ids: Vec<String>,
        all_visible: bool,
    ) -> Result<usize, String> {
        let Some(session) = self.notifications_sessions.get(&handle.key) else {
            return Err("notification session is not open".to_string());
        };
        let changed = if all_visible {
            session.projection.mark_all_read()
        } else {
            session.projection.mark_read(event_ids)
        };
        Ok(changed)
    }

    pub(crate) fn close_notifications_session(
        &mut self,
        handle: BrowserNotificationsSessionHandle,
    ) {
        self.close_notifications_key(&handle.key);
    }

    pub(crate) fn close_notifications_key(&mut self, session_id: &str) {
        let Some(session) = self.notifications_sessions.remove(session_id) else {
            return;
        };
        for id in session.observer_ids {
            self.observed_projection_registrar.close(id);
        }
        self.runtime
            .reducer
            .remove_snapshot_projection(&session.projection_key);
    }

    fn register_notifications_sidecar(
        &mut self,
        key: &str,
        projection: Arc<NotificationsProjection>,
    ) {
        let key_for_row = key.to_string();
        self.runtime
            .reducer
            .register_typed_snapshot_projection(key.to_string(), move || {
                let snapshot = projection.snapshot();
                Some(TypedProjectionData {
                    key: key_for_row.clone(),
                    schema_id: NOTIFICATIONS_SCHEMA_ID.to_string(),
                    schema_version: NOTIFICATIONS_SCHEMA_VERSION,
                    file_identifier: notifications_file_identifier(),
                    payload: encode_notifications_snapshot(&snapshot),
                    ..Default::default()
                })
            });
    }
}

fn notifications_key(session_id: &str) -> String {
    if session_id.is_empty() {
        NOTIFICATIONS_KEY.to_string()
    } else {
        format!("{NOTIFICATIONS_KEY}.{session_id}")
    }
}

fn notifications_consumer(session_id: &str, pubkey: &str) -> String {
    format!("notifications-{session_id}-{pubkey}")
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
