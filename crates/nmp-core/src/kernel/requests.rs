use super::*;

impl Kernel {
    pub(crate) fn relay_connecting(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "connecting".to_string();
        self.changed_since_emit = true;
        self.log(format!("connecting {} relay {}", role.key(), role.url()));
    }

    pub(crate) fn relay_connected(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "connected".to_string();
        relay.connected_at = Some(Instant::now());
        relay.last_error = None;
        self.changed_since_emit = true;
        self.log(format!("{} relay connected", role.key()));
    }

    pub(crate) fn relay_failed(&mut self, role: RelayRole, error: String) {
        let relay = self.relay_mut(role);
        relay.connection = "backing_off".to_string();
        relay.last_error = Some(truncate(&error, 160));
        relay.reconnect_count = relay.reconnect_count.saturating_add(1);
        self.thread_ids_inflight = false;
        self.thread_replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!(
            "{} relay error: {}",
            role.key(),
            truncate(&error, 140)
        ));
        for sub in self.wire_subs.values_mut() {
            if sub.role == role && sub.state != "closed" {
                sub.state = "retrying".to_string();
            }
        }
    }

    pub(crate) fn relay_closed(&mut self, role: RelayRole) {
        self.relay_mut(role).connection = "closed".to_string();
        for sub in self.wire_subs.values_mut() {
            if sub.role == role {
                sub.state = "closed".to_string();
            }
        }
        self.changed_since_emit = true;
    }

    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));
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
            "target kind:0 profile via indexer",
            json!({"kinds":[0],"authors":[TEST_PUBKEY],"limit":1}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "target-relays",
            "target NIP-65 relay list",
            json!({"kinds":[10002],"authors":[TEST_PUBKEY],"limit":1}),
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
        self.requested_profiles.insert(TEST_PUBKEY.to_string());
        for seed in seed_accounts() {
            self.requested_profiles.insert(seed.pubkey.to_string());
        }
        requests
    }

    pub(crate) fn active_subscriptions(&self, role: RelayRole) -> Vec<String> {
        self.wire_subs
            .values()
            .filter(|sub| {
                sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            })
            .map(|sub| sub.id.clone())
            .collect()
    }

    pub(crate) fn open_author(&mut self, pubkey: String, can_send: bool) -> Vec<OutboundMessage> {
        match self.selected_author.as_mut() {
            Some(interest) if interest.key == pubkey => {
                interest.refcount = interest.refcount.saturating_add(1);
            }
            _ => {
                self.selected_author = Some(ViewInterest {
                    key: pubkey.clone(),
                    refcount: 1,
                });
            }
        }
        self.author_request_pending = true;
        self.changed_since_emit = true;
        self.log(format!("open author view {}", short_hex(&pubkey)));

        if can_send {
            self.author_requests()
        } else {
            self.log("author view request queued until relay connects");
            Vec::new()
        }
    }

    pub(crate) fn open_thread(&mut self, event_id: String, can_send: bool) -> Vec<OutboundMessage> {
        match self.selected_thread.as_mut() {
            Some(interest) if interest.key == event_id => {
                interest.refcount = interest.refcount.saturating_add(1);
            }
            _ => {
                self.selected_thread = Some(ViewInterest {
                    key: event_id.clone(),
                    refcount: 1,
                });
                self.pending_thread_ids.clear();
                self.requested_thread_ids.clear();
                self.pending_thread_reply_targets.clear();
                self.requested_thread_reply_targets.clear();
            }
        }
        self.thread_request_pending = true;
        self.changed_since_emit = true;
        self.log(format!("open thread view {}", short_hex(&event_id)));

        if can_send {
            self.prepare_thread_requests()
        } else {
            self.log("thread request queued until relay connects");
            Vec::new()
        }
    }

    pub(crate) fn open_firehose_tag(
        &mut self,
        tag: String,
        can_send: bool,
    ) -> Vec<OutboundMessage> {
        let tag = tag.trim().trim_start_matches('#').to_lowercase();
        if tag.is_empty() {
            return Vec::new();
        }
        match self.diagnostic_firehose.as_mut() {
            Some(interest) if interest.key == tag => {
                interest.refcount = interest.refcount.saturating_add(1);
                return Vec::new();
            }
            _ => {
                self.diagnostic_firehose = Some(ViewInterest {
                    key: tag.clone(),
                    refcount: 1,
                });
            }
        }
        self.changed_since_emit = true;
        self.log(format!("open diagnostic firehose #{tag}"));

        if can_send {
            self.firehose_requests()
        } else {
            self.log("diagnostic firehose queued until relay connects");
            Vec::new()
        }
    }

    pub(crate) fn claim_profile(
        &mut self,
        pubkey: String,
        consumer_id: String,
        can_send: bool,
    ) -> Vec<OutboundMessage> {
        let (inserted, refcount) = {
            let consumers = self.profile_claims.entry(pubkey.clone()).or_default();
            let inserted = consumers.insert(consumer_id.clone());
            (inserted, consumers.len())
        };
        if inserted {
            self.log(format!(
                "claim profile {} consumer {} ref {}",
                short_hex(&pubkey),
                truncate(&consumer_id, 80),
                refcount
            ));
        }
        self.changed_since_emit = true;

        if self.profiles.contains_key(&pubkey)
            || self.requested_profiles.contains(&pubkey)
            || self.pending_profiles.contains(&pubkey)
        {
            return Vec::new();
        }

        if can_send {
            self.profile_claim_request(pubkey)
        } else {
            self.pending_profiles.insert(pubkey);
            self.log("profile claim queued until indexer connects");
            Vec::new()
        }
    }

    pub(crate) fn release_profile(
        &mut self,
        pubkey: &str,
        consumer_id: &str,
    ) -> Vec<OutboundMessage> {
        let mut remove_claim = false;
        let mut remaining = 0;
        if let Some(consumers) = self.profile_claims.get_mut(pubkey) {
            consumers.remove(consumer_id);
            remaining = consumers.len();
            remove_claim = consumers.is_empty();
        }
        if remove_claim {
            self.profile_claims.remove(pubkey);
            self.pending_profiles.remove(pubkey);
        }
        self.changed_since_emit = true;
        self.log(format!(
            "release profile {} consumer {} ref {}",
            short_hex(pubkey),
            truncate(consumer_id, 80),
            remaining
        ));
        Vec::new()
    }

    pub(crate) fn close_author(&mut self, pubkey: &str) -> Vec<OutboundMessage> {
        let Some(interest) = self.selected_author.as_mut() else {
            return Vec::new();
        };
        if interest.key != pubkey {
            return Vec::new();
        }
        interest.refcount = interest.refcount.saturating_sub(1);
        if interest.refcount > 0 {
            self.changed_since_emit = true;
            return Vec::new();
        }

        self.selected_author = None;
        self.author_request_pending = false;
        self.changed_since_emit = true;
        self.log(format!("close author view {}", short_hex(pubkey)));
        self.close_subscriptions_with_prefixes(&[
            "author-profile-",
            "author-notes-",
            "author-relays-",
        ])
    }

    pub(crate) fn close_thread(&mut self, event_id: &str) -> Vec<OutboundMessage> {
        let Some(interest) = self.selected_thread.as_mut() else {
            return Vec::new();
        };
        if interest.key != event_id {
            return Vec::new();
        }
        interest.refcount = interest.refcount.saturating_sub(1);
        if interest.refcount > 0 {
            self.changed_since_emit = true;
            return Vec::new();
        }

        self.selected_thread = None;
        self.thread_request_pending = false;
        self.pending_thread_ids.clear();
        self.pending_thread_reply_targets.clear();
        self.thread_ids_inflight = false;
        self.thread_replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!("close thread view {}", short_hex(event_id)));
        self.close_subscriptions_with_prefixes(&["thread-ids-", "thread-replies-", "thread-more-"])
    }

    pub(super) fn close_subscriptions_with_prefixes(
        &mut self,
        prefixes: &[&str],
    ) -> Vec<OutboundMessage> {
        let mut closes = Vec::new();
        for sub in self.wire_subs.values_mut() {
            if prefixes.iter().any(|prefix| sub.id.starts_with(prefix))
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            {
                sub.state = "closed".to_string();
                sub.close_reason = Some("view closed".to_string());
                closes.push(OutboundMessage {
                    role: sub.role,
                    text: json!(["CLOSE", sub.id]).to_string(),
                });
            }
        }
        closes
    }

    pub(crate) fn pending_view_requests(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        while let Some(message) = self.deferred_outbound.pop_front() {
            requests.push(message);
        }
        if self.author_request_pending {
            requests.extend(self.author_requests());
        }
        if self.thread_request_pending {
            requests.extend(self.prepare_thread_requests());
        }
        if self.diagnostic_firehose.is_some()
            && !self
                .wire_subs
                .keys()
                .any(|sub_id| sub_id.starts_with("diag-firehose-"))
        {
            requests.extend(self.firehose_requests());
        }
        requests.extend(self.pending_profile_claim_requests());
        requests.extend(self.maybe_open_thread_hydration());
        requests
    }

    pub(super) fn firehose_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(tag) = self
            .diagnostic_firehose
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            return Vec::new();
        };
        self.diagnostic_firehose_seq = self.diagnostic_firehose_seq.saturating_add(1);
        vec![self.req(
            RelayRole::Content,
            &format!("diag-firehose-{}", self.diagnostic_firehose_seq),
            &format!("diagnostic hashtag firehose #{tag}"),
            json!({"kinds":[1],"#t":[tag],"limit":500}),
        )]
    }

    pub(super) fn pending_profile_claim_requests(&mut self) -> Vec<OutboundMessage> {
        let authors = self.pending_profiles.iter().cloned().collect::<Vec<_>>();
        let mut requests = Vec::new();
        for author in authors {
            if self.profile_claims.contains_key(&author)
                && !self.profiles.contains_key(&author)
                && !self.requested_profiles.contains(&author)
            {
                requests.extend(self.profile_claim_request(author));
            } else {
                self.pending_profiles.remove(&author);
            }
        }
        requests
    }

    pub(super) fn profile_claim_request(&mut self, pubkey: String) -> Vec<OutboundMessage> {
        self.pending_profiles.remove(&pubkey);
        if self.profiles.contains_key(&pubkey) || !self.requested_profiles.insert(pubkey.clone()) {
            return Vec::new();
        }
        self.profile_req_seq = self.profile_req_seq.saturating_add(1);
        vec![self.req(
            RelayRole::Indexer,
            &format!("profile-claim-{}", self.profile_req_seq),
            &format!("claimed UI profile {}", short_hex(&pubkey)),
            json!({"kinds":[0],"authors":[pubkey],"limit":1}),
        )]
    }

    pub(super) fn author_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(pubkey) = self
            .selected_author
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            self.author_request_pending = false;
            return Vec::new();
        };

        self.author_request_pending = false;
        self.author_view_seq = self.author_view_seq.saturating_add(1);
        self.requested_profiles.insert(pubkey.clone());
        let mut requests = vec![
            self.req(
                RelayRole::Indexer,
                &format!("author-relays-{}", self.author_view_seq),
                &format!("selected author NIP-65 {}", short_hex(&pubkey)),
                json!({"kinds":[10002],"authors":[pubkey.clone()],"limit":1}),
            ),
            self.req(
                RelayRole::Indexer,
                &format!("author-profile-{}", self.author_view_seq),
                &format!("selected author kind:0 {}", short_hex(&pubkey)),
                json!({"kinds":[0],"authors":[pubkey.clone()],"limit":1}),
            ),
            self.req(
                RelayRole::Content,
                &format!("author-notes-{}", self.author_view_seq),
                &format!("selected author notes {}", short_hex(&pubkey)),
                json!({"kinds":[1,6],"authors":[pubkey],"limit":100}),
            ),
        ];
        requests.append(&mut self.maybe_open_thread_hydration());
        requests
    }

    pub(super) fn prepare_thread_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(focused_id) = self
            .selected_thread
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            self.thread_request_pending = false;
            return Vec::new();
        };

        self.thread_request_pending = false;
        let root_id = self
            .thread_root_id(&focused_id)
            .unwrap_or_else(|| focused_id.clone());
        self.enqueue_thread_id(focused_id.clone());
        self.enqueue_thread_id(root_id.clone());
        self.enqueue_thread_reply_target(root_id);
        self.enqueue_thread_reply_target(focused_id.clone());
        if let Some(focused) = self.events.get(&focused_id) {
            for id in referenced_event_ids(focused) {
                self.enqueue_thread_id(id.clone());
                self.enqueue_thread_reply_target(id);
            }
        }
        self.maybe_open_thread_hydration()
    }

    pub(super) fn enqueue_thread_id(&mut self, id: String) {
        if is_hex_id(&id) && !self.requested_thread_ids.contains(&id) {
            self.pending_thread_ids.insert(id);
        }
    }

    pub(super) fn enqueue_thread_reply_target(&mut self, id: String) {
        if is_hex_id(&id)
            && self.requested_thread_reply_targets.len() < 96
            && !self.requested_thread_reply_targets.contains(&id)
        {
            self.pending_thread_reply_targets.insert(id);
        }
    }

    pub(super) fn maybe_open_thread_hydration(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        if !self.pending_thread_ids.is_empty() && !self.thread_ids_inflight {
            let ids = self
                .pending_thread_ids
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.pending_thread_ids.remove(id);
                self.requested_thread_ids.insert(id.clone());
            }
            self.thread_view_seq = self.thread_view_seq.saturating_add(1);
            self.thread_ids_inflight = true;
            requests.push(self.req(
                RelayRole::Content,
                &format!("thread-ids-{}", self.thread_view_seq),
                "thread context ids",
                json!({"ids":ids,"limit":20}),
            ));
        }

        if !self.pending_thread_reply_targets.is_empty() && !self.thread_replies_inflight {
            let ids = self
                .pending_thread_reply_targets
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.pending_thread_reply_targets.remove(id);
                self.requested_thread_reply_targets.insert(id.clone());
            }
            self.thread_view_seq = self.thread_view_seq.saturating_add(1);
            self.thread_replies_inflight = true;
            requests.push(self.req(
                RelayRole::Content,
                &format!("thread-replies-{}", self.thread_view_seq),
                "thread recursive replies",
                json!({"kinds":[1,6],"#e":ids,"limit":200}),
            ));
        }

        requests
    }

    pub(super) fn req(
        &mut self,
        role: RelayRole,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> OutboundMessage {
        self.log(format!("REQ {sub_id}@{}: {summary}", role.key()));
        self.wire_subs.insert(
            sub_id.to_string(),
            WireSub {
                id: sub_id.to_string(),
                role,
                filter_summary: summary.to_string(),
                state: "opening".to_string(),
                opened_at: Instant::now(),
                last_event_at: None,
                eose_at: None,
                close_reason: None,
            },
        );
        self.changed_since_emit = true;
        OutboundMessage {
            role,
            text: json!(["REQ", sub_id, filter]).to_string(),
        }
    }

    pub(crate) fn defer_outbound(&mut self, message: OutboundMessage) {
        self.log(format!(
            "defer {} outbound until relay reconnects",
            message.role.key()
        ));
        self.deferred_outbound.push_back(message);
        while self.deferred_outbound.len() > 64 {
            self.deferred_outbound.pop_front();
        }
        self.changed_since_emit = true;
    }

    pub(crate) fn record_tx(&mut self, role: RelayRole, bytes: usize) {
        let relay = self.relay_mut(role);
        relay.counters.bytes_tx = relay.counters.bytes_tx.saturating_add(bytes as u64);
    }
}
