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
//! - `pending_profile_claim_requests` → disappears (compiler handles deferred relay reconnect)
//! - `open_firehose_tag`   → register LogicalInterest { kinds:[1], tags:{#t:[tag]} }
//! - `firehose_requests`   → disappears (replaced by open_firehose_tag registration)
//!
//! The `req()` helper and `RelayRole`-based routing are replaced by the
//! wire-emitter's `emit_req(relay_url, sub_id, filter)` call.

use super::super::*;

impl Kernel {
    pub(crate) fn open_author(
        &mut self,
        pubkey: String,
        can_send: bool,
    ) -> Vec<OutboundMessage> {
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

    pub(crate) fn firehose_requests(&mut self) -> Vec<OutboundMessage> {
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

    pub(crate) fn pending_profile_claim_requests(&mut self) -> Vec<OutboundMessage> {
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

    pub(crate) fn profile_claim_request(&mut self, pubkey: String) -> Vec<OutboundMessage> {
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

    pub(crate) fn author_requests(&mut self) -> Vec<OutboundMessage> {
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
}
