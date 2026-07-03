//! Input-intent classification + dispatch runtime API.
//!
//! `nmp-intent` owns pure classification. The native runtime owns the
//! side-effecting dispatch decision because it names the `NmpApp` handle,
//! actor sender, optional search sessions, and NIP-05 protocol command lane.
//! C ABI crates parse C strings and serialize the returned value only.

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget,
};

use crate::NmpApp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputIntentDispatch {
    Dispatched(InputIntentCandidate),
    Rejection(InputIntentRejection),
}

impl NmpApp {
    /// Classify one input-intent request against the app-registered recognizers.
    #[must_use]
    pub fn classify_input_intent(&self, request: &InputIntentRequest) -> InputIntentClassification {
        let recognizers = self.input_scope_recognizers();
        nmp_intent::classify(request, &recognizers)
    }

    /// Classify and dispatch the top candidate through the runtime-owned seam.
    ///
    /// `RelayUrl` and `Registered` candidates intentionally have no generic
    /// runtime side effect. They are returned as dispatched for the host or
    /// owning recognizer to route.
    #[must_use]
    pub fn dispatch_input_intent(
        &self,
        request: &InputIntentRequest,
        session_id: Option<&str>,
    ) -> InputIntentDispatch {
        match self.classify_input_intent(request) {
            InputIntentClassification::Rejection(rejection) => {
                InputIntentDispatch::Rejection(rejection)
            }
            InputIntentClassification::Candidates(candidates) => {
                match candidates.into_iter().next() {
                    // The classifier normally emits a `Rejection` instead of an
                    // empty candidate list. Stay total if a future recognizer path
                    // changes that invariant.
                    None => InputIntentDispatch::Rejection(InputIntentRejection::Unparseable),
                    Some(candidate) => {
                        self.act_on_input_intent_candidate(&candidate, session_id);
                        InputIntentDispatch::Dispatched(candidate)
                    }
                }
            }
        }
    }

    fn act_on_input_intent_candidate(
        &self,
        candidate: &InputIntentCandidate,
        session_id: Option<&str>,
    ) {
        match &candidate.target {
            InputIntentTarget::DirectRef { uri } => {
                self.send_cmd(ActorCommand::Kernel(nmp_core::KernelAction::OpenUri {
                    uri: uri.clone(),
                }));
            }
            InputIntentTarget::TextQuery { request_json } => {
                self.act_on_text_query_intent(request_json, session_id);
            }
            InputIntentTarget::Nip05 { identifier } => {
                if let Some((name, domain)) = nmp_nip05::parse_nip05(identifier) {
                    self.send_cmd(ActorCommand::Protocol(Box::new(
                        nmp_nip05::ResolveNip05Command {
                            name,
                            domain,
                            correlation_id: None,
                        },
                    )));
                }
            }
            InputIntentTarget::RelayUrl { .. } | InputIntentTarget::Registered { .. } => {}
        }
    }

    #[cfg(feature = "search")]
    fn act_on_text_query_intent(&self, request_json: &str, session_id: Option<&str>) {
        if let Some(session_id) = session_id.filter(|s| !s.is_empty()) {
            if let Some(request) = nmp_nip50::parse_search_request(request_json) {
                let _ = nmp_nip50::open_search(
                    self,
                    nmp_nip50::Nip50SearchSession::new(request, session_id),
                );
            }
        }
    }

    #[cfg(not(feature = "search"))]
    fn act_on_text_query_intent(&self, _request_json: &str, _session_id: Option<&str>) {}
}
