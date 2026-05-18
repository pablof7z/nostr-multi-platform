//! Kind:1 / kind:6 (note / repost) timeline ingest.
//!
//! Covers event storage, deduplication, timeline ordering, thread hydration
//! queue management, and the seed-timeline open gate.

use super::super::*;
use super::event_short_id;
use std::hash::{Hash, Hasher};

/// Stable per-relay sub-id for the follow-feed REQ. The `seed-timeline-`
/// prefix is recognized as a long-lived (post-EOSE keep-alive) subscription
/// in `ingest::handle_text`. The hash suffix is a deterministic 8-char tag
/// over the relay URL — same URL → same sub-id across runs, so wire-sub
/// identity in the diagnostic surface is stable.
fn timeline_sub_id_for(relay_url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    relay_url.hash(&mut hasher);
    format!("seed-timeline-{:08x}", (hasher.finish() & 0xFFFF_FFFF))
}

impl Kernel {
    /// Ingest a kind:1 or kind:6 event into the local read-cache and timeline.
    ///
    /// Routes through `EventStore::insert` (D4 single-writer).  On `Inserted |
    /// Replaced`, populates the lightweight `events` read-cache and appends to
    /// `timeline`.  On `Duplicate`, updates `relay_count` from the authoritative
    /// provenance count in the store.  All other outcomes (Superseded, Tombstoned,
    /// Rejected, Ephemeral) are dropped.
    pub(in crate::kernel) fn ingest_timeline_event(
        &mut self,
        _role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        event: NostrEvent,
    ) {
        if !self.should_store_event(sub_id, &event) {
            return;
        }

        // D4: route through EventStore for ALL deliveries, including duplicates.
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
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                ));
                return;
            }
        };
        // T105: provenance is the resolved per-author write relay the EVENT
        // actually arrived on, not the lane's bootstrap URL.
        let provenance = relay_url.to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let proceed = match self.store.insert(verified, &provenance, received_at_ms) {
            Ok(outcome) => {
                use crate::store::InsertOutcome;
                match outcome {
                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => true,
                    InsertOutcome::Duplicate { sources_after, .. } => {
                        if let Some(cached) = self.events.get_mut(&event.id) {
                            cached.relay_count = sources_after;
                        }
                        return;
                    }
                    InsertOutcome::Superseded { .. } => return,
                    InsertOutcome::Tombstoned { .. }
                    | InsertOutcome::Rejected { .. }
                    | InsertOutcome::Ephemeral { .. } => return,
                }
            }
            Err(e) => {
                self.log(format!("store insert error: {e}"));
                if self.events.contains_key(&event.id) {
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

        // T82 discovery seam (notedeck §3.10): collect referenced-but-missing
        // pubkeys/event ids (p/e/q tags) into UnknownIds *before* `event.tags`
        // is moved into the cache — borrowed visitor, no clone, zero alloc
        // when every reference is already cached (D8). The actor turns the
        // deduped set into OneshotApi fetches via `drain_unknown_oneshots`.
        self.collect_unknown_refs(&event.tags);

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
        self.changed_since_emit = true;
    }

    pub(in crate::kernel) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
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
            // T82: a discovered quoted-note / referenced event arrives on its
            // oneshot sub — it must be stored so the missing reference is
            // actually resolved (otherwise the next ingest re-discovers it).
            || sub_id.starts_with(crate::kernel::discovery::ONESHOT_SUB_PREFIX)
    }

    pub(in crate::kernel) fn enqueue_thread_hydration_from_event(&mut self, event_id: &str) {
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

    pub(in crate::kernel) fn sort_timeline(&mut self) {
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

    /// Open the seed-timeline subscription once enough contacts are loaded.
    ///
    /// Returns REQ messages to send; also flushes pending profile claim requests.
    pub(in crate::kernel) fn maybe_open_timeline(&mut self) -> Vec<OutboundMessage> {
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
            let authors_vec = self.timeline_authors.iter().cloned().collect::<Vec<_>>();
            self.timeline_requested = true;
            self.timeline_opened_at = Some(Instant::now());

            // T105: partition the follow set by each author's NIP-65 write
            // relays — one REQ per resolved relay, carrying only the authors
            // that relay actually serves. Cold-start authors (no cached
            // kind:10002) land on the bootstrap discovery seed; the A1
            // recompilation trigger re-emits the timeline onto resolved
            // relays once kind:10002 arrives (see `ingest_relay_list`).
            let partition = self.partition_authors_by_write_relays(&authors_vec);
            self.log(format!(
                "opening seed timeline: {} authors fanned out over {} relay(s)",
                authors_vec.len(),
                partition.len()
            ));

            // Stable sub-id per relay: `seed-timeline-<short-hash>`. The
            // bare `seed-timeline` id is reserved for the unpartitioned
            // legacy path and would now conflict across relays.
            for (relay_url, served_authors) in partition {
                let sub_id = timeline_sub_id_for(&relay_url);
                requests.push(self.req_for_relay(
                    RelayRole::Content,
                    relay_url,
                    &sub_id,
                    "seed union timeline kinds:1,6 (NIP-65 outbox)",
                    json!({"kinds":[1,6],"authors":served_authors,"limit":200}),
                ));
            }
        }

        requests.extend(self.pending_profile_claim_requests());
        requests
    }

    pub(in crate::kernel) fn should_open_timeline(&self) -> bool {
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
