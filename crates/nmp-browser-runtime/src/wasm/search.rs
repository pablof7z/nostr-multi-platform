// Search dispatch is a browser-runtime protocol adapter: the worker request
// shape is mapped into the generic NIP-50 Rust request, then the kernel-owned
// runtime performs the actual search lifecycle.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::BTreeSet;

use super::core::NmpRuntimeCore;
use super::dispatch_support::not_started_error;
use super::protocol::{SearchClose, SearchOpen, SearchScope, SearchTargets, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_search_open(&mut self, req: SearchOpen) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        let Some(request) = search_request_from_protocol(&req) else {
            return vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.nip50.search.open".to_string(),
                correlation_id: req.correlation_id,
                reason: "invalid_search_request".to_string(),
            }];
        };
        let _ = nmp_nip50::open_search(
            &*handle,
            nmp_nip50::Nip50SearchSession::new(request, req.session_id),
        );
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip50.search.open".to_string(),
            correlation_id: req.correlation_id,
        }]
    }

    pub(super) fn handle_search_close(&mut self, req: SearchClose) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };
        nmp_nip50::close_search(
            &*handle,
            &nmp_nip50::Nip50SearchHandle::for_key(req.session_id),
        );
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.nip50.search.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}

fn search_request_from_protocol(req: &SearchOpen) -> Option<nmp_nip50::SearchRequest> {
    let scope = match req.scope {
        SearchScope::Notes => {
            nmp_nip50::SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE]))
        }
        SearchScope::Profiles => nmp_nip50::SearchScope::Users,
        SearchScope::Longform => nmp_nip50::SearchScope::LongForm,
    };
    let targets = match req.targets {
        SearchTargets::UserPreferred => nmp_nip50::SearchTargets::UserPreferred,
        SearchTargets::AppDefault => nmp_nip50::SearchTargets::AppDefault,
        SearchTargets::Explicit => nmp_nip50::SearchTargets::Explicit(req.relays.clone()),
    };
    nmp_nip50::SearchRequest::new(&req.query, scope, targets, req.max_hits)
}
