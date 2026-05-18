//! Cold-start REQ emission: the seed-author bootstrap timeline, self profile
//! / NIP-65 relay list, and the seed-author kind:0/kind:3/kind:10002 fan-out
//! that primes the indexer cache before any view opens.

use super::super::*;

impl Kernel {
    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));

        // Use the active account as the "self" target for profile/relay-list
        // lookups. Falls back to TEST_PUBKEY in anonymous/demo mode (no
        // persistence yet, so this branch fires when sign-in precedes the
        // first relay connection).
        let self_pk = self
            .active_account
            .clone()
            .unwrap_or_else(|| TEST_PUBKEY.to_string());

        let seeds = seed_accounts();
        let seed_pubkeys = seeds.iter().map(|seed| seed.pubkey).collect::<Vec<_>>();

        for seed in &seeds {
            self.timeline_authors.insert(seed.pubkey.to_string());
            self.log(format!(
                "seed account: {} {}",
                seed.name,
                short_hex(seed.pubkey)
            ));
        }

        let mut requests = Vec::new();
        requests.push(self.req(
            RelayRole::Content,
            "seed-bootstrap",
            "seed author bootstrap timeline",
            json!({"kinds":[1,6],"authors":seed_pubkeys.clone(),"limit":80}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "profile-target",
            "self kind:0 profile via indexer",
            json!({"kinds":[0],"authors":[self_pk],"limit":1}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "target-relays",
            "self NIP-65 relay list",
            json!({"kinds":[10002],"authors":[self_pk],"limit":1}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-contacts",
            "seed kind:3 contacts via indexer",
            json!({"kinds":[3],"authors":seed_pubkeys.clone(),"limit":10}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-profiles",
            "seed kind:0 profiles via indexer",
            json!({"kinds":[0],"authors":seed_pubkeys.clone(),"limit":20}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-relays",
            "seed NIP-65 relay lists",
            json!({"kinds":[10002],"authors":seed_pubkeys,"limit":10}),
        ));
        self.requested_profiles.insert(self_pk);
        for seed in seed_accounts() {
            self.requested_profiles.insert(seed.pubkey.to_string());
        }
        requests
    }
}
