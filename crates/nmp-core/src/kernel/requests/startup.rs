//! Cold-start REQ emission: self profile / NIP-65 relay list, and the active
//! account's kind:3 follow list. No hardcoded seed timeline.

use super::super::*;

impl Kernel {
    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));
        self.active_account_bootstrap_requests()
    }

    /// Emit profile + relay-list + contacts REQs for the currently active
    /// account. Called at cold-start (via `startup_requests`) and again after
    /// sign-in / account creation / switch when the active account changes.
    pub(crate) fn active_account_bootstrap_requests(&mut self) -> Vec<OutboundMessage> {
        let self_pk = match &self.active_account {
            Some(pk) => pk.clone(),
            None => return Vec::new(),
        };

        let mut requests = Vec::new();
        requests.extend(self.req(
            RelayRole::Indexer,
            "profile-target",
            "self kind:0 profile via indexer",
            json!({"kinds":[0],"authors":[self_pk],"limit":1}),
        ));
        requests.extend(self.req(
            RelayRole::Indexer,
            "target-relays",
            "self NIP-65 relay list",
            json!({"kinds":[10002],"authors":[self_pk],"limit":1}),
        ));
        requests.extend(self.req(
            RelayRole::Indexer,
            "self-contacts",
            "self kind:3 contacts via indexer",
            json!({"kinds":[3],"authors":[self_pk],"limit":1}),
        ));
        self.requested_profiles.insert(self_pk);
        requests
    }
}
