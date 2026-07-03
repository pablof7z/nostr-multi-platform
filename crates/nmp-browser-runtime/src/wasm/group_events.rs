// NIP-29 group-events dispatch is a browser-runtime protocol adapter.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::core::NmpRuntimeCore;
use super::dispatch_support::not_started_error;
use super::protocol::{GroupEventsClose, GroupEventsOpen, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_group_events_open(&mut self, req: GroupEventsOpen) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        let relay = req.relay_url.trim().to_string();
        let local_id = req.group_id.trim().to_string();
        if relay.is_empty() {
            return vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_events.open".to_string(),
                correlation_id: req.correlation_id,
                reason: "group events relay_url is required".to_string(),
            }];
        }
        if local_id.is_empty() {
            return vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_events.open".to_string(),
                correlation_id: req.correlation_id,
                reason: "group events group_id is required".to_string(),
            }];
        }

        let group = nmp_nip29::GroupId::new(relay, local_id);
        if let Err(reason) = group.require_routable() {
            return vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_events.open".to_string(),
                correlation_id: req.correlation_id,
                reason: format!("invalid group events group_id: {reason}"),
            }];
        }

        let _ = nmp_nip29::open_nip29_group_events_session(
            &*handle,
            nmp_nip29::Nip29GroupEventsSession::new(group, req.kinds),
        );
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_events.open".to_string(),
            correlation_id: req.correlation_id,
        }]
    }

    pub(super) fn handle_group_events_close(&mut self, req: GroupEventsClose) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        let _ = req.session_id;
        let _ = nmp_nip29::close_nip29_group_events_read_by_key(&*handle);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_events.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
