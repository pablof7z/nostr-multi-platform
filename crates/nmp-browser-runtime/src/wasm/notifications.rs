//! Notification dispatch is a browser-runtime protocol adapter.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::core::NmpRuntimeCore;
use super::dispatch::not_started_error;
use super::protocol::{NotificationsClose, NotificationsMarkRead, NotificationsOpen, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_notifications_open(&mut self, req: NotificationsOpen) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        match handle.open_notifications(&req.account_pubkey, &req.session_id) {
            Ok(_) => vec![WorkerEvent::ActionAccepted {
                action_type: "nmp.relations.notifications.open".to_string(),
                correlation_id: req.correlation_id,
            }],
            Err(reason) => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.relations.notifications.open".to_string(),
                correlation_id: req.correlation_id,
                reason,
            }],
        }
    }

    pub(super) fn handle_notifications_close(
        &mut self,
        req: NotificationsClose,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        handle.close_notifications(&req.session_id);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.relations.notifications.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }

    pub(super) fn handle_notifications_mark_read(
        &mut self,
        req: NotificationsMarkRead,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        match handle.mark_notifications_read(&req.session_id, req.event_ids, req.all_visible) {
            Ok(_) => vec![WorkerEvent::ActionAccepted {
                action_type: "nmp.relations.notifications.mark_read".to_string(),
                correlation_id: req.correlation_id,
            }],
            Err(reason) => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.relations.notifications.mark_read".to_string(),
                correlation_id: req.correlation_id,
                reason,
            }],
        }
    }
}
