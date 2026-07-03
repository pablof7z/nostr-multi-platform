//! Input-intent classification + dispatch runtime API.
//!
//! `nmp-intent` owns pure classification. The native runtime owns the
//! side-effecting dispatch decision because it names the `NmpApp` handle,
//! actor sender, optional search sessions, and optional protocol command lanes.
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
                let mut iter = candidates.into_iter();
                match iter.next() {
                    // The classifier normally emits a `Rejection` instead of an
                    // empty candidate list. Stay total if a future recognizer path
                    // changes that invariant.
                    None => InputIntentDispatch::Rejection(InputIntentRejection::Unparseable),
                    Some(primary) => {
                        // D1 (#2927): a NIP-AD candidate is emitted alongside the
                        // free-text search candidates for the same input. Fire
                        // those in parallel so the user is never blocked on the AD
                        // `.well-known` fetch — an AD URL still yields search
                        // results today; a direct AD-resolved view is a strictly
                        // later upgrade.
                        if matches!(primary.target, InputIntentTarget::AdCandidate { .. }) {
                            for sibling in iter {
                                if matches!(sibling.target, InputIntentTarget::TextQuery { .. }) {
                                    self.act_on_input_intent_candidate(&sibling, session_id);
                                }
                            }
                        }
                        self.act_on_input_intent_candidate(&primary, session_id);
                        InputIntentDispatch::Dispatched(primary)
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
                self.act_on_nip05_intent(identifier);
            }
            InputIntentTarget::AdCandidate { .. } => {
                // #2927 moment-2: the AD `.well-known` resolution + relay-pinned
                // collection delivery is the deferred hand-off — it needs the
                // arbitrary-filter, relay-pinned collection interest primitive
                // the kernel does not yet expose (no `KernelAction` /
                // `ActorCommand` opens a plain-`nostr::Filter` interest with
                // one-shot `relay_pin` into a 0..N-event view; the owner override
                // forbids reducing a NIP-AD filter to the single-pointer refs
                // seam). The parallel free-text search (D1) is dispatched from
                // `dispatch_input_intent`, so an AD URL still yields results; only
                // the direct AD-resolved collection view is pending.
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

    #[cfg(feature = "nip05")]
    fn act_on_nip05_intent(&self, identifier: &str) {
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

    #[cfg(not(feature = "nip05"))]
    fn act_on_nip05_intent(&self, _identifier: &str) {}
}
