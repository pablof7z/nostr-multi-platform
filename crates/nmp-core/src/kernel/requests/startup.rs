//! Cold-start REQ emission: self profile / NIP-65 relay list / NIP-17 DM relay
//! list, and the active account's kind:3 follow list. No hardcoded seed timeline.

use super::super::*;

impl Kernel {
    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));
        self.active_account_bootstrap_requests()
    }

    /// Emit profile + relay-list + DM-relay-list + contacts REQs for the
    /// currently active account. Called at cold-start (via `startup_requests`)
    /// and again after sign-in / account creation / switch when the active
    /// account changes.
    ///
    /// F-02: kind:10050 (NIP-17 DM relay list) is fetched here so that
    /// existing users see their DM inbox subscription open immediately on
    /// sign-in instead of waiting for the DM runtime to publish its own
    /// kind:10050 and round-trip it back through the relay. Without this,
    /// `dm_relay_lists` is empty at sign-in and the `PTagRouting::Nip17DmRelays`
    /// routing for the gift-wrap inbox interest fails-closed until the
    /// publish→ingest round-trip closes — a structural latency wart for any
    /// user who already has a kind:10050 published on a prior device.
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
            "self-dm-relays",
            "self NIP-17 DM relay list",
            json!({"kinds":[10050],"authors":[self_pk],"limit":1}),
        ));
        requests.extend(self.req(
            RelayRole::Indexer,
            "self-contacts",
            "self kind:3 contacts via indexer",
            json!({"kinds":[3],"authors":[self_pk],"limit":1}),
        ));
        self.profile_requests.requested.insert(self_pk);
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// Extract the REQ frames whose sub-id matches `sub_id`.
    fn reqs_with_sub_id<'a>(
        msgs: &'a [OutboundMessage],
        sub_id: &str,
    ) -> Vec<&'a OutboundMessage> {
        let needle = format!("[\"REQ\",\"{sub_id}\"");
        msgs.iter().filter(|m| m.text.starts_with(&needle)).collect()
    }

    /// F-02: active-account bootstrap must emit a kind:10050 REQ tagged
    /// `self-dm-relays` on the Indexer lane for the active account, alongside
    /// the existing kind:0 / kind:10002 / kind:3 self fetches. Without this,
    /// existing users wait for a publish→ingest round-trip before the NIP-17
    /// DM inbox subscription can open against their declared DM relays.
    #[test]
    fn bootstrap_emits_kind_10050_dm_relay_list_req_for_active_account() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.active_account = Some(ALICE.to_string());

        let msgs = kernel.active_account_bootstrap_requests();

        // (1) Exactly one logical bootstrap REQ for kind:10050 was emitted
        // (the `req` helper fans out one frame per configured indexer-lane
        // bootstrap URL — all carry the same `self-dm-relays` sub-id and
        // identical filter, so we assert on at least one and lock the
        // filter shape below).
        let dm_relay_reqs = reqs_with_sub_id(&msgs, "self-dm-relays");
        assert!(
            !dm_relay_reqs.is_empty(),
            "active-account bootstrap must emit a kind:10050 REQ tagged \
             `self-dm-relays`; got: {:#?}",
            msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
        );

        // (2) The REQ filter shape is exactly the F-02 spec:
        //     {"kinds":[10050],"authors":[self_pk],"limit":1}
        // Parse the wire text to avoid string-substring false-positives.
        for req in &dm_relay_reqs {
            let parsed: Value = serde_json::from_str(&req.text)
                .expect("REQ frame must be valid JSON");
            let arr = parsed.as_array().expect("REQ frame must be a JSON array");
            assert_eq!(arr[0], json!("REQ"));
            assert_eq!(arr[1], json!("self-dm-relays"));
            let filter = &arr[2];
            assert_eq!(
                filter["kinds"],
                json!([10050]),
                "self-dm-relays filter must target kind:10050 only"
            );
            assert_eq!(
                filter["authors"],
                json!([ALICE]),
                "self-dm-relays filter must scope to the active account pubkey"
            );
            assert_eq!(
                filter["limit"],
                json!(1),
                "self-dm-relays filter limit must be 1 (replaceable event)"
            );
        }

        // (3) The pre-existing bootstrap REQs are still emitted — the F-02
        // patch is additive, not a replacement. This locks the four-block
        // shape so a future regression that drops kind:0 / kind:10002 /
        // kind:3 in pursuit of the kind:10050 add is caught here.
        assert!(
            !reqs_with_sub_id(&msgs, "profile-target").is_empty(),
            "kind:0 self-profile REQ must still be emitted"
        );
        assert!(
            !reqs_with_sub_id(&msgs, "target-relays").is_empty(),
            "kind:10002 self NIP-65 REQ must still be emitted"
        );
        assert!(
            !reqs_with_sub_id(&msgs, "self-contacts").is_empty(),
            "kind:3 self-contacts REQ must still be emitted"
        );
    }

    /// Without an active account, bootstrap is a no-op — the existing
    /// contract (line 18: early return on `None`) must continue to hold,
    /// including for the new kind:10050 path. Pins the negative case so a
    /// future "always fetch" refactor that ignores `active_account` is caught.
    #[test]
    fn bootstrap_emits_no_dm_relay_list_req_without_active_account() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.active_account = None;

        let msgs = kernel.active_account_bootstrap_requests();

        assert!(
            msgs.is_empty(),
            "active-account bootstrap must be a no-op with no active account; \
             got: {:#?}",
            msgs.iter().map(|m| &m.text).collect::<Vec<_>>()
        );
    }
}
