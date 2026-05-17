use super::*;

impl Kernel {
    pub(crate) fn handle_message(
        &mut self,
        role: RelayRole,
        message: Message,
    ) -> Vec<OutboundMessage> {
        match message {
            Message::Text(text) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
                self.handle_text(role, &text)
            }
            Message::Binary(bytes) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx =
                    relay.counters.bytes_rx.saturating_add(bytes.len() as u64);
                Vec::new()
            }
            Message::Ping(_) | Message::Pong(_) => Vec::new(),
            Message::Close(frame) => {
                let relay = self.relay_mut(role);
                relay.connection = "closed".to_string();
                relay.last_error = frame.map(|frame| frame.reason.to_string());
                self.changed_since_emit = true;
                Vec::new()
            }
            Message::Frame(_) => Vec::new(),
        }
    }

    pub(super) fn handle_text(&mut self, role: RelayRole, text: &str) -> Vec<OutboundMessage> {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            self.log(format!("unparseable relay frame: {}", truncate(text, 120)));
            return Vec::new();
        };

        let Some(array) = value.as_array() else {
            return Vec::new();
        };

        let Some(kind) = array.first().and_then(Value::as_str) else {
            return Vec::new();
        };

        let mut outbound = Vec::new();
        match kind {
            "EVENT" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                if let Some(event_value) = array.get(2) {
                    self.handle_event(role, sub_id, event_value);
                }
            }
            "EOSE" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                {
                    let relay = self.relay_mut(role);
                    relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
                }
                if let Some(sub) = self.wire_subs.get_mut(sub_id) {
                    sub.state = if sub_id == "seed-timeline" || sub_id.starts_with("diag-firehose-")
                    {
                        "live".to_string()
                    } else {
                        "closed".to_string()
                    };
                    sub.eose_at = Some(Instant::now());
                }
                if sub_id.starts_with("profiles-") {
                    self.profile_req_inflight = false;
                }
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
                }
                if sub_id != "seed-timeline" && !sub_id.starts_with("diag-firehose-") {
                    outbound.push(OutboundMessage {
                        role,
                        text: json!(["CLOSE", sub_id]).to_string(),
                    });
                }
                self.changed_since_emit = true;
                self.log(format!("EOSE {sub_id}"));
            }
            "NOTICE" => {
                let notice = array
                    .get(1)
                    .and_then(Value::as_str)
                    .map(|s| truncate(s, 180))
                    .unwrap_or_else(|| "notice".to_string());
                let relay = self.relay_mut(role);
                relay.counters.notices_rx = relay.counters.notices_rx.saturating_add(1);
                relay.last_notice = Some(notice.clone());
                self.changed_since_emit = true;
                self.log(format!("NOTICE {} {notice}", role.key()));
            }
            "CLOSED" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                let reason = array
                    .get(2)
                    .and_then(Value::as_str)
                    .map(|s| truncate(s, 180));
                {
                    let relay = self.relay_mut(role);
                    relay.counters.closed_rx = relay.counters.closed_rx.saturating_add(1);
                }
                if let Some(sub) = self.wire_subs.get_mut(sub_id) {
                    sub.state = "closed_by_relay".to_string();
                    sub.close_reason = reason.clone();
                }
                if sub_id.starts_with("profiles-") {
                    self.profile_req_inflight = false;
                }
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
                }
                self.changed_since_emit = true;
                self.log(format!(
                    "CLOSED {sub_id} {}",
                    reason.unwrap_or_else(|| "".to_string())
                ));
            }
            "OK" => {}
            _ => self.log(format!("relay frame {kind}")),
        }

        outbound.extend(self.maybe_open_timeline());
        outbound.extend(self.maybe_open_thread_hydration());
        outbound
    }

    pub(super) fn handle_event(&mut self, role: RelayRole, sub_id: &str, value: &Value) {
        let Ok(event) = serde_json::from_value::<NostrEvent>(value.clone()) else {
            self.log(format!("bad EVENT payload on {sub_id}"));
            return;
        };

        let now = Instant::now();
        {
            let relay = self.relay_mut(role);
            relay.counters.events_rx = relay.counters.events_rx.saturating_add(1);
            relay.last_event_at = Some(now);
        }
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.last_event_at = Some(now);
        self.first_event_at.get_or_insert(now);
        if let Some(sub) = self.wire_subs.get_mut(sub_id) {
            if sub.state == "opening" {
                sub.state = "live".to_string();
            }
            sub.last_event_at = Some(now);
        }

        match event.kind {
            0 => self.ingest_profile(event),
            1 | 6 => self.ingest_timeline_event(sub_id, event),
            3 => self.ingest_contacts(event),
            10002 => self.ingest_relay_list(event),
            _ => {}
        }
        self.changed_since_emit = true;
    }

    pub(super) fn ingest_profile(&mut self, event: NostrEvent) {
        let candidate = parse_profile(&event);
        let should_replace = self
            .profiles
            .get(&event.pubkey)
            .map(|current| {
                candidate.created_at > current.created_at
                    || (candidate.created_at == current.created_at
                        && candidate.event_id < current.event_id)
            })
            .unwrap_or(true);

        if should_replace {
            if event.pubkey == TEST_PUBKEY {
                self.target_profile_loaded_at
                    .get_or_insert_with(Instant::now);
            }
            self.profiles.insert(event.pubkey, candidate);
        }
    }

    pub(super) fn ingest_contacts(&mut self, event: NostrEvent) {
        let follows = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().map(String::as_str) == Some("p") {
                    tag.get(1).filter(|value| is_hex_pubkey(value)).cloned()
                } else {
                    None
                }
            })
            .take(TIMELINE_AUTHOR_LIMIT)
            .collect::<Vec<_>>();

        self.log(format!(
            "contacts {} -> {} followees",
            short_hex(&event.pubkey),
            follows.len()
        ));
        self.seed_contacts.insert(event.pubkey, follows);
    }

    pub(super) fn ingest_relay_list(&mut self, event: NostrEvent) {
        let relay_list = parse_relay_list(event.created_at, &event.tags);
        if relay_list.read_relays.is_empty()
            && relay_list.write_relays.is_empty()
            && relay_list.both_relays.is_empty()
        {
            return;
        }

        let should_replace = self
            .author_relay_lists
            .get(&event.pubkey)
            .map(|current| relay_list.created_at >= current.created_at)
            .unwrap_or(true);
        if should_replace {
            self.log(format!(
                "NIP-65 {} read={} write={} both={}",
                short_hex(&event.pubkey),
                relay_list.read_relays.len(),
                relay_list.write_relays.len(),
                relay_list.both_relays.len()
            ));
            self.author_relay_lists.insert(event.pubkey, relay_list);
        }
    }

    pub(super) fn ingest_timeline_event(&mut self, sub_id: &str, event: NostrEvent) {
        if self.events.contains_key(&event.id) {
            if let Some(stored) = self.events.get_mut(&event.id) {
                stored.relay_count = stored.relay_count.saturating_add(1);
            }
            return;
        }

        if !self.should_store_event(sub_id, &event) {
            return;
        }

        let stored = StoredEvent {
            id: event.id.clone(),
            author: event.pubkey.clone(),
            kind: event.kind,
            created_at: event.created_at,
            tags: event.tags,
            content: event.content,
            relay_count: 1,
        };
        self.events.insert(event.id.clone(), stored);
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose_events = self.diagnostic_firehose_events.saturating_add(1);
        }
        self.enqueue_thread_hydration_from_event(&event.id);
        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
            self.timeline.push_back(event.id);
            self.sort_timeline();
            self.timeline_first_item_at.get_or_insert_with(Instant::now);
        }
        if !self.profiles.contains_key(&event.pubkey)
            && !self.requested_profiles.contains(&event.pubkey)
        {
            self.pending_profiles.insert(event.pubkey);
        }
    }

    pub(super) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
        self.timeline_authors.contains(&event.pubkey)
            || self
                .selected_author
                .as_ref()
                .map(|interest| interest.key == event.pubkey)
                .unwrap_or(false)
            || sub_id.starts_with("author-notes-")
            || sub_id.starts_with("thread-ids-")
            || sub_id.starts_with("thread-replies-")
            || sub_id.starts_with("diag-firehose-")
    }

    pub(super) fn enqueue_thread_hydration_from_event(&mut self, event_id: &str) {
        let Some(selected) = self
            .selected_thread
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            return;
        };
        let Some(event) = self.events.get(event_id).cloned() else {
            return;
        };
        let root = self
            .thread_root_id(&selected)
            .unwrap_or_else(|| selected.clone());
        let is_related = event.id == selected
            || event.id == root
            || event_references(&event, &selected)
            || event_references(&event, &root);
        if !is_related {
            return;
        }

        self.enqueue_thread_reply_target(event.id.clone());
        for id in referenced_event_ids(&event) {
            self.enqueue_thread_id(id.clone());
            self.enqueue_thread_reply_target(id);
        }
    }

    pub(super) fn sort_timeline(&mut self) {
        let mut ids = self.timeline.iter().cloned().collect::<Vec<_>>();
        ids.sort_by(|left, right| {
            let a = self
                .events
                .get(left)
                .map(|event| event.created_at)
                .unwrap_or(0);
            let b = self
                .events
                .get(right)
                .map(|event| event.created_at)
                .unwrap_or(0);
            b.cmp(&a).then_with(|| left.cmp(right))
        });
        ids.truncate(500);
        self.timeline = ids.into();
    }

    pub(super) fn maybe_open_timeline(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        if !self.timeline_requested && self.should_open_timeline() {
            let mut authors = BTreeSet::new();
            for seed in seed_accounts() {
                authors.insert(seed.pubkey.to_string());
            }
            for follows in self.seed_contacts.values() {
                for follow in follows {
                    authors.insert(follow.clone());
                    if authors.len() >= TIMELINE_AUTHOR_LIMIT {
                        break;
                    }
                }
                if authors.len() >= TIMELINE_AUTHOR_LIMIT {
                    break;
                }
            }
            self.timeline_authors = authors;
            let authors = self.timeline_authors.iter().cloned().collect::<Vec<_>>();
            self.timeline_requested = true;
            self.timeline_opened_at = Some(Instant::now());
            self.log(format!(
                "opening seed timeline with {} authors",
                self.timeline_authors.len()
            ));
            requests.push(self.req(
                RelayRole::Content,
                "seed-timeline",
                "seed union timeline kinds:1,6",
                json!({"kinds":[1,6],"authors":authors,"limit":200}),
            ));
        }

        if !self.pending_profiles.is_empty() && !self.profile_req_inflight {
            let authors = self
                .pending_profiles
                .iter()
                .take(PROFILE_REQ_BATCH)
                .cloned()
                .collect::<Vec<_>>();
            for author in &authors {
                self.requested_profiles.insert(author.clone());
                self.pending_profiles.remove(author);
            }
            self.profile_req_seq = self.profile_req_seq.saturating_add(1);
            self.profile_req_inflight = true;
            let sub_id = format!("profiles-{}", self.profile_req_seq);
            requests.push(self.req(
                RelayRole::Indexer,
                &sub_id,
                "visible timeline kind:0 profiles via indexer",
                json!({"kinds":[0],"authors":authors,"limit":PROFILE_REQ_BATCH}),
            ));
        }

        requests
    }

    pub(super) fn should_open_timeline(&self) -> bool {
        if self.timeline_requested {
            return false;
        }

        let seed_count = seed_accounts().len();
        self.seed_contacts.len() >= seed_count
            || self
                .contacts_deadline
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(false)
    }
}
