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
        let relay = req.relay_url.trim().to_string();
        if relay.is_empty() {
            return vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip29.group_discovery.open".to_string(),
                correlation_id: req.correlation_id,
                reason: "group discovery relay_url is required".to_string(),
            }];
        }

        let _ = nmp_nip29::open_nip29_group_discovery_session(
            &*handle,
            nmp_nip29::Nip29GroupDiscoverySession::new(relay),
        );
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_discovery.open".to_string(),
            correlation_id: req.correlation_id,
        }]
    }

    pub(super) fn handle_group_discovery_close(
        &mut self,
        req: GroupDiscoveryClose,
    ) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        let _ = req.session_id;
        let _ = nmp_nip29::close_nip29_group_discovery_read_by_key(&*handle);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip29.group_discovery.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
