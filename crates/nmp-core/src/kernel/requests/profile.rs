//! Profile, author, and diagnostic-firehose request builders.
//!
//! # M2 migration plan (compiler.md §3.5)
//! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
//! builders are scheduled for replacement by `SubscriptionCompiler`-driven
//! interest registration once the wire-emitter, InterestRegistry, and
//! trigger-based recompilation infrastructure land (M2 full migration):
//!
//! - `open_author`         → register three LogicalInterests; call compiler.recompile()
//! - `claim_profile`       → register LogicalInterest { kinds:[0], limit:1 }; dedup via registry
//! - `release_profile`     → unregister LogicalInterest by InterestId
//! - `close_author`        → drop interests by InterestId; recompile(Trigger::ViewClose)
//! - `author_requests`     → disappears (replaced by open_author interest registration)
//! - `profile_claim_request` → disappears (compiler routes via Stage 1+2)
//! - `pending_profile_requests` → disappears (compiler handles deferred relay reconnect)
//! - `open_firehose_tag`   → register LogicalInterest { kinds:[1], tags:{#t:[tag]} }
//! - `firehose_requests`   → disappears (replaced by open_firehose_tag registration)
//!
//! The `req()` helper and `RelayRole`-based routing are replaced by the
//! wire-emitter's `emit_req(relay_url, sub_id, filter)` call.

use super::super::*;
use crate::stable_hash::stable_hash64;

/// Stable 8-hex-char suffix for a relay URL — used to disambiguate fan-out
/// sub-ids across resolved relays so the `wire_subs` map (keyed by sub-id)
/// does not collapse N per-relay subscriptions onto one row.
fn relay_tag(relay_url: &str) -> String {
    format!(
        "{:08x}",
        stable_hash64(("profile-relay-tag", relay_url)) & 0xFFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_tag_is_restart_stable() {
        assert_eq!(relay_tag("wss://relay.example"), "0684d673");
        assert_eq!(
            relay_tag("wss://relay.example"),
            relay_tag("wss://relay.example")
        );
        assert_ne!(
            relay_tag("wss://relay.example"),
            relay_tag("wss://other.example")
        );
    }
}

impl Kernel {
    pub(crate) fn open_author(&mut self, pubkey: String, can_send: bool) -> Vec<OutboundMessage> {
        match self.author_view.selected_author.as_mut() {
            Some(interest) if interest.key == pubkey => {
                interest.refcount = interest.refcount.saturating_add(1);
            }
            _ => {
                self.author_view.selected_author = Some(ViewInterest {
                    key: pubkey.clone(),
                    refcount: 1,
                });
            }
        }
        self.author_view.request_pending = true;
        self.changed_since_emit = true;
        self.log(format!("open author view {}", short_hex(&pubkey)));

        if can_send {
            self.author_requests()
        } else {
            self.log("author view request queued until relay connects");
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
        match self.diagnostic_firehose.interest.as_mut() {
            Some(interest) if interest.key == tag => {
                interest.refcount = interest.refcount.saturating_add(1);
                return Vec::new();
            }
            _ => {
                self.diagnostic_firehose.interest = Some(ViewInterest {
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
        // T114b — per-pubkey claim consumer-id retention bound. Without this
        // check the BTreeSet grows once per `claim_profile` call (S2 mix:
        // unique consumer_id per dispatch, no matching release) and per-dispatch
        // retention scales with dispatch count rather than working-set size —
        // a D8 violation (`docs/perf/m10.5/s2-drain-analysis.md`). Drop-newest
        // on overflow mirrors the bounded actor channel; the dropped claim
        // becomes a silent no-op (D6: never an FFI error) and bumps the
        // diagnostic counter `claim_drops_total`.
        let (inserted, refcount) = {
            let consumers = self.profile_claims.entry(pubkey.clone()).or_default();
            if !consumers.contains(&consumer_id)
                && consumers.len() >= super::super::MAX_CLAIMS_PER_PUBKEY
            {
                self.claim_drops_total = self.claim_drops_total.saturating_add(1);
                // hot path
                return Vec::new();
            }
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
            || self.profile_requests.requested.contains(&pubkey)
            || self.profile_requests.pending.contains(&pubkey)
        {
            return Vec::new();
        }

        if can_send {
            self.profile_claim_request(pubkey)
        } else {
            self.profile_requests.pending.insert(pubkey);
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
            self.profile_requests.pending.remove(pubkey);
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

    pub(in crate::kernel) fn request_profile_for_rendered_note(&mut self, pubkey: &str) {
        if self.profiles.contains_key(pubkey)
            || self.profile_requests.requested.contains(pubkey)
            || self.profile_requests.pending.contains(pubkey)
        {
            return;
        }

        self.profile_requests.pending.insert(pubkey.to_string());
        self.changed_since_emit = true;
        self.log(format!("queue note author profile {}", short_hex(pubkey)));
    }

    pub(crate) fn close_author(&mut self, pubkey: &str) -> Vec<OutboundMessage> {
        let Some(interest) = self.author_view.selected_author.as_mut() else {
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

        self.author_view.selected_author = None;
        self.author_view.request_pending = false;
        self.changed_since_emit = true;
        self.log(format!("close author view {}", short_hex(pubkey)));
        self.close_subscriptions_with_prefixes(&[
            "author-profile-",
            "author-notes-",
            "author-relays-",
        ])
    }

    pub(crate) fn firehose_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(tag) = self
            .diagnostic_firehose
            .interest
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            return Vec::new();
        };
        self.diagnostic_firehose.seq = self.diagnostic_firehose.seq.saturating_add(1);
        let seq = self.diagnostic_firehose.seq;

        // T122 / codex R2: hashtag firehose REQs are inbox-direction (D3) —
        // the user IS the recipient of their own hashtag interest, so the
        // routing destination is the active account's NIP-65 *read* relays
        // (kind:10002 read + both markers). Cold-start (no active account
        // selected, or no kind:10002 cached) falls back to the bootstrap
        // discovery seed via `recipient_read_relays`/`bootstrap_discovery_relays`.
        let relays = match self.active_account.clone() {
            Some(pubkey) => self.recipient_read_relays(&pubkey),
            None => self.bootstrap_discovery_relays(),
        };

        relays
            .into_iter()
            .map(|relay_url| {
                let tag_suffix = relay_tag(&relay_url);
                self.req_for_relay(
                    RelayRole::Content,
                    relay_url,
                    &format!("diag-firehose-{seq}-{tag_suffix}"),
                    &format!("diagnostic hashtag firehose #{tag}"),
                    json!({"kinds":[1],"#t":[tag],"limit":500}),
                )
            })
            .collect()
    }

    pub(crate) fn pending_profile_claim_requests(&mut self) -> Vec<OutboundMessage> {
        // Collect valid pending authors: not already fetched/inflight.
        let authors: Vec<String> = self
            .profile_requests
            .pending
            .iter()
            .filter(|pk| {
                !self.profiles.contains_key(*pk) && !self.profile_requests.requested.contains(*pk)
            })
            .cloned()
            .collect();

        if authors.is_empty() {
            // Evict any pending authors already satisfied or requested.
            self.profile_requests.pending.retain(|pk| {
                !self.profiles.contains_key(pk) && !self.profile_requests.requested.contains(pk)
            });
            return Vec::new();
        }

        // Group authors by relay — cold-start → INDEXER only; NIP-65 known → write relays.
        let mut by_relay: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for author in &authors {
            for relay_url in self.author_indexer_relays(author) {
                by_relay.entry(relay_url).or_default().push(author.clone());
            }
        }

        // Mark all as requested and remove from pending.
        for author in &authors {
            self.profile_requests.pending.remove(author);
            self.profile_requests.requested.insert(author.clone());
        }

        // One batched REQ per relay with all authors in a single `authors` array.
        self.profile_requests.req_seq = self.profile_requests.req_seq.saturating_add(1);
        let seq = self.profile_requests.req_seq;
        let mut requests = Vec::new();
        for (relay_url, relay_authors) in by_relay {
            let tag = relay_tag(&relay_url);
            let n = relay_authors.len();
            requests.push(self.req_for_relay(
                RelayRole::Indexer,
                relay_url,
                &format!("profile-batch-{seq}-{tag}"),
                &format!("batched profile claims ({n})"),
                json!({"kinds":[0],"authors": relay_authors,"limit": n}),
            ));
        }
        requests
    }

    pub(crate) fn profile_claim_request(&mut self, pubkey: String) -> Vec<OutboundMessage> {
        self.profile_requests.pending.remove(&pubkey);
        if self.profiles.contains_key(&pubkey)
            || !self.profile_requests.requested.insert(pubkey.clone())
        {
            return Vec::new();
        }
        self.profile_requests.req_seq = self.profile_requests.req_seq.saturating_add(1);
        let seq = self.profile_requests.req_seq;
        // T105: kind:0 is a discovery fetch. Cold-start → INDEXER relay only.
        // NIP-65 known → declared write relays (the author published kind:0
        // there). Never send cold-start profile claims to the content relay.
        let mut requests = Vec::new();
        for relay_url in self.author_indexer_relays(&pubkey) {
            let tag = relay_tag(&relay_url);
            requests.push(self.req_for_relay(
                RelayRole::Indexer,
                relay_url,
                &format!("profile-claim-{seq}-{tag}"),
                &format!("claimed UI profile {}", short_hex(&pubkey)),
                json!({"kinds":[0],"authors":[pubkey.clone()],"limit":1}),
            ));
        }
        requests
    }

    pub(crate) fn author_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(pubkey) = self
            .author_view
            .selected_author
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            self.author_view.request_pending = false;
            return Vec::new();
        };

        self.author_view.request_pending = false;
        self.author_view.seq = self.author_view.seq.saturating_add(1);
        self.profile_requests.requested.insert(pubkey.clone());
        let seq = self.author_view.seq;

        // T105: kind:10002 + kind:0 are discovery fetches — the author's own
        // relay list is what we're trying to learn, so they leave on the
        // bootstrap discovery seeds (one per indexer-lane bootstrap relay).
        // kind:1/6 (the author's notes) MUST go to the author's resolved
        // NIP-65 write relays — this is the outbox direction; the author
        // publishes there. If we don't yet have their kind:10002 cached,
        // the bootstrap seed serves the cold-start fetch and the A1
        // recompilation trigger re-emits onto resolved relays after
        // ingest_relay_list lands the author's kind:10002.
        let mut requests = Vec::new();
        for seed in self.bootstrap_discovery_relays() {
            let tag = relay_tag(&seed);
            requests.push(self.req_for_relay(
                RelayRole::Indexer,
                seed.clone(),
                &format!("author-relays-{seq}-{tag}"),
                &format!("selected author NIP-65 {}", short_hex(&pubkey)),
                json!({"kinds":[10002],"authors":[pubkey.clone()],"limit":1}),
            ));
            requests.push(self.req_for_relay(
                RelayRole::Indexer,
                seed,
                &format!("author-profile-{seq}-{tag}"),
                &format!("selected author kind:0 {}", short_hex(&pubkey)),
                json!({"kinds":[0],"authors":[pubkey.clone()],"limit":1}),
            ));
        }
        for relay_url in self.author_write_relays(&pubkey) {
            let tag = relay_tag(&relay_url);
            requests.push(self.req_for_relay(
                RelayRole::Content,
                relay_url,
                &format!("author-notes-{seq}-{tag}"),
                &format!("selected author notes {}", short_hex(&pubkey)),
                json!({"kinds":[1,6],"authors":[pubkey.clone()],"limit":100}),
            ));
        }
        requests.append(&mut self.maybe_open_thread_hydration());
        requests
    }
}
