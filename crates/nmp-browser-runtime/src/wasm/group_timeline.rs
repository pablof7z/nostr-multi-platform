// NIP-29 group timeline dispatch is a browser-runtime protocol adapter.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::core::NmpRuntimeCore;
use super::dispatch::not_started_error;
use super::protocol::{GroupTimelineClose, GroupTimelineOpen, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_group_timeline_open(
        &mut self,
        req: GroupTimelineOpen,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        match handle.open_group_timeline(&req.relay_url, &req.group_id, &req.session_id) {
            Ok(_) => vec![WorkerEvent::ActionAccepted {
                action_type: "nmp.nip29.group_timeline.open".to_string(),
                correlation_id: req.correlation_id,
            }],
            Err(reason) => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_timeline.open".to_string(),
                correlation_id: req.correlation_id,
                reason,
            }],
        }
    }

    pub(super) fn handle_group_timeline_close(
        &mut self,
        req: GroupTimelineClose,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        handle.close_group_timeline(&req.session_id);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_timeline.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
