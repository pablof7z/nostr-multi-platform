// NIP-29 group discovery dispatch is a browser-runtime protocol adapter.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use super::core::NmpRuntimeCore;
use super::dispatch_support::not_started_error;
use super::protocol::{GroupDiscoveryClose, GroupDiscoveryOpen, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_group_discovery_open(
        &mut self,
        req: GroupDiscoveryOpen,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        match handle.open_group_discovery(&req.relay_url, &req.session_id) {
            Ok(_) => vec![WorkerEvent::ActionAccepted {
                action_type: "nmp.nip29.group_discovery.open".to_string(),
                correlation_id: req.correlation_id,
            }],
            Err(reason) => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_discovery.open".to_string(),
                correlation_id: req.correlation_id,
                reason,
            }],
        }
    }

    pub(super) fn handle_group_discovery_close(
        &mut self,
        req: GroupDiscoveryClose,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        handle.close_group_discovery(&req.session_id);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_discovery.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
