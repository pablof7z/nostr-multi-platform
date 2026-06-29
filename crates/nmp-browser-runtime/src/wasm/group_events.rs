// NIP-29 group-events dispatch is a browser-runtime protocol adapter.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::core::NmpRuntimeCore;
use super::dispatch_support::not_started_error;
use super::protocol::{GroupEventsClose, GroupEventsOpen, WorkerEvent};
use crate::runtime::BrowserGroupEventsSessionDescriptor;

impl NmpRuntimeCore {
    pub(super) fn handle_group_events_open(&mut self, req: GroupEventsOpen) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        let descriptor = BrowserGroupEventsSessionDescriptor {
            relay_url: req.relay_url,
            group_id: req.group_id,
            kinds: req.kinds,
            session_id: req.session_id,
        };
        match handle.open_nip29_group_events_session(descriptor) {
            Ok(_) => vec![WorkerEvent::ActionAccepted {
                action_type: "nmp.nip29.group_events.open".to_string(),
                correlation_id: req.correlation_id,
            }],
            Err(reason) => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_events.open".to_string(),
                correlation_id: req.correlation_id,
                reason,
            }],
        }
    }

    pub(super) fn handle_group_events_close(&mut self, req: GroupEventsClose) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        handle.close_nip29_group_events_session_by_id(&req.session_id);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_events.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
