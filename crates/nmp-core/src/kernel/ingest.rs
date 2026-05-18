use super::*;

/// Returns up to the first 16 chars of an event id, safe for any length.
fn event_short_id(id: &str) -> &str {
    &id[..id.len().min(16)]
}

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

        // D4: all events are persisted before kind-specific dispatch.
        // Kinds 1|6 handle their own store.insert inside ingest_timeline_event
        // (which also manages the local read-cache); all other kinds are
        // persisted here so the store is the single authoritative writer.
        //
        // For replaceable kinds (0, 3, 10002) we gate local cache mutations on
        // the store outcome: only Inserted | Replaced means this event is now
        // canonical — Superseded / Duplicate / Tombstoned / Rejected must not
        // overwrite a newer entry already held by the store (D4).
        match event.kind {
            1 | 6 => self.ingest_timeline_event(role, sub_id, event),
            0 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(outcome, Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })) {
                    self.ingest_profile(event);
                }
                self.changed_since_emit = true;
            }
            3 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(outcome, Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })) {
                    self.ingest_contacts(event);
                }
                self.changed_since_emit = true;
            }
            10002 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(outcome, Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })) {
                    self.ingest_relay_list(event);
                }
                self.changed_since_emit = true;
            }
            _ => {
                self.verify_and_persist(role, &event);
                self.changed_since_emit = true;
            }
        }
    }

    /// Verify and persist an event to the EventStore.
    ///
    /// Returns `Some(outcome)` with the store's [`InsertOutcome`] when
    /// verification succeeds, or `None` when signature verification fails.
    /// Callers that perform local-cache mutations for replaceable kinds **must**
    /// inspect the outcome: only `Inserted | Replaced` means this event is now
    /// the canonical version in the store — all other outcomes must be treated
    /// as no-ops for cache purposes (D4).
    pub(super) fn verify_and_persist(
        &mut self,
        role: RelayRole,
        event: &NostrEvent,
    ) -> Option<crate::store::InsertOutcome> {
        let raw = crate::store::RawEvent {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            created_at: event.created_at,
            kind: event.kind,
            tags: event.tags.clone(),
            content: event.content.clone(),
            sig: event.sig.clone(),
        };
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
            Ok(v) => v,
            Err(e) => {
                self.log(format!("sig verify failed for {}: {e}", event_short_id(&event.id)));
                return None;
            }
        };
        let relay_url = role.url().to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        match self.store.insert(verified, &relay_url, received_at_ms) {
            Ok(outcome) => Some(outcome),
            Err(e) => {
                self.log(format!("store insert error for {}: {e}", event_short_id(&event.id)));
                None
            }
        }
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
        let relay_list = parse_relay_list(&event.id, event.created_at, &event.tags);
        if relay_list.read_relays.is_empty()
            && relay_list.write_relays.is_empty()
            && relay_list.both_relays.is_empty()
        {
            return;
        }

        // This function is only called after verify_and_persist returned
        // Inserted | Replaced, so the store already enforced strict `>` with
        // lexicographic event-id tiebreak. The local cache guard below is a
        // belt-and-suspenders check that mirrors the store's supersession
        // logic exactly (strict `>` on timestamp; same-ts resolved by
        // lexicographically smaller event id wins).
        let should_replace = self
            .author_relay_lists
            .get(&event.pubkey)
            .map(|current| {
                relay_list.created_at > current.created_at
                    || (relay_list.created_at == current.created_at
                        && event.id < current.event_id)
            })
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

    pub(super) fn ingest_timeline_event(&mut self, role: RelayRole, sub_id: &str, event: NostrEvent) {
        if !self.should_store_event(sub_id, &event) {
            return;
        }

        // D4: route through EventStore (the single writer) for ALL deliveries,
        // including duplicates. This ensures provenance is updated on every
        // relay re-delivery, not just the first one.
        //
        // Signature verification via VerifiedEvent::try_from_raw. Events that
        // fail verification are logged and dropped — not cached locally.
        let raw = crate::store::RawEvent {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            created_at: event.created_at,
            kind: event.kind,
            tags: event.tags.clone(),
            content: event.content.clone(),
            sig: event.sig.clone(),
        };
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
            Ok(v) => v,
            Err(e) => {
                self.log(format!("sig verify failed for {}: {e}", event_short_id(&event.id)));
                return;
            }
        };
        let relay_url = role.url().to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Store insert; log but don't abort on error (graceful degradation).
        // Returns false to signal "skip local cache population".
        let proceed = match self.store.insert(verified, &relay_url, received_at_ms) {
            Ok(outcome) => {
                use crate::store::InsertOutcome;
                match outcome {
                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => {
                        // Genuinely new or replaced event — populate read-cache.
                        true
                    }
                    InsertOutcome::Duplicate { sources_after, .. } => {
                        // Store already has this event; update relay_count from
                        // the authoritative provenance count, not a local increment.
                        if let Some(cached) = self.events.get_mut(&event.id) {
                            cached.relay_count = sources_after;
                        }
                        // Provenance updated in store; no new timeline entry needed.
                        return;
                    }
                    InsertOutcome::Superseded { .. } => {
                        // Incoming was older than what we have; discard.
                        return;
                    }
                    InsertOutcome::Tombstoned { .. } | InsertOutcome::Rejected { .. }
                    | InsertOutcome::Ephemeral { .. } => {
                        // Store rejected the event; skip populating local cache.
                        return;
                    }
                }
            }
            Err(e) => {
                self.log(format!("store insert error: {e}"));
                // Graceful degradation: fall through to local-cache-only path.
                if self.events.contains_key(&event.id) {
                    // Already cached locally — update relay_count heuristically.
                    if let Some(cached) = self.events.get_mut(&event.id) {
                        cached.relay_count = cached.relay_count.saturating_add(1);
                    }
                    return;
                }
                true
            }
        };

        if !proceed {
            return;
        }

        // Populate the lightweight read-cache for timeline ordering + display.
        let cached = StoredEvent {
            id: event.id.clone(),
            author: event.pubkey.clone(),
            kind: event.kind,
            created_at: event.created_at,
            tags: event.tags,
            content: event.content,
            relay_count: 1,
        };
        self.events.insert(event.id.clone(), cached);
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose_events = self.diagnostic_firehose_events.saturating_add(1);
        }
        self.enqueue_thread_hydration_from_event(&event.id);
        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
            self.timeline.push_back(event.id);
            self.sort_timeline();
            self.timeline_first_item_at.get_or_insert_with(Instant::now);
        }
        // Only set changed_since_emit on genuinely new/replaced events (Inserted/Replaced outcomes).
        self.changed_since_emit = true;
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

        requests.extend(self.pending_profile_claim_requests());
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
