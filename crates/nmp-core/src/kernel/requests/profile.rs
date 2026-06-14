//! Profile, and diagnostic-firehose request builders.
//!
//! V-68 / V-112 (ADR-0042): `open_author`, `close_author`, `author_requests`
//! deleted. Author view state now lives in the per-app FlatFeed registered by
//! `nmp_app_chirp_open_author_feed`. Profile claim/release remain here.
//!
//! # Debt A — routing through the substrate router
//!
//! V-51 phase 5 (PR #462) added an observe-only `observe_subscription_through_router`
//! shim that fired the router for the trace projection but kept the actual
//! REQ-construction flowing through `Kernel::author_write_relays` /
//! `recipient_read_relays` / `author_indexer_relays` cache helpers — the
//! substrate router was wired but never trusted to make the routing
//! decision. Debt A (this commit) deletes that half-step: every per-author
//! dispatch site in this file now consumes
//! [`Kernel::route_outbox_subscription_relays`] (outbox-direction:
//! author-published kinds 0/1/6/10002 routed against the author's NIP-65
//! write set via `outbox_router.route_publish`) or
//! [`Kernel::route_subscription_relays`] (inbox-direction: hashtag
//! firehose routed against the active account's NIP-65 read set via
//! `outbox_router.route_subscription`) — both call the kernel's
//! `outbox_router` slot and return the routed URL set directly. The
//! router's trace observer fires automatically on success — no separate
//! observation call is needed.
//!
//! The cold-start bootstrap seed flows through the substrate seam at
//! [`crate::substrate::SessionKeySet::app_relays`] (lane 7 fallback):
//!
//! * `BootstrapSeed::Discovery` (indexer + content combined) — kind:1/6
//!   author notes, kind:10002 author NIP-65 probe (cold-start fan-out),
//!   hashtag firehose.
//! * `BootstrapSeed::IndexerOnly` — kind:0 profile-claim discovery (the
//!   historical `author_indexer_relays` contract — profile-claim REQs
//!   must not leak onto the shared content relay at cold-start).
//!
//! # M2 migration plan (compiler.md §3.5)
//! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
//! builders are scheduled for replacement by `SubscriptionCompiler`-driven
//! interest registration once the wire-emitter, `InterestRegistry`, and
//! trigger-based recompilation infrastructure land (M2 full migration):
//!
//! - `claim_profile`       → register `LogicalInterest` { kinds:[0], limit:1 }; dedup via registry
//! - `release_profile`     → unregister `LogicalInterest` by `InterestId`
//! - `profile_claim_request` → disappears (compiler routes via Stage 1+2)
//! - `pending_profile_requests` → disappears (compiler handles deferred relay reconnect)
//! - `open_firehose_tag` / `firehose_requests` → DONE (ADR-0042, V-112 PR2):
//!   the bespoke kernel methods were deleted; a hashtag feed is now a generic
//!   `open_interest` ({"kinds":[1],"#t":[tag]}, scope Global) routed through the
//!   `SubscriptionCompiler` like every other subscription.
//!
//! The `req()` helper and `RelayRole`-based routing are replaced by the
//! wire-emitter's `emit_req(relay_url, sub_id, filter)` call.

use super::super::mailboxes::BootstrapSeed;
use super::super::{json, short_hex, truncate, Kernel, OutboundMessage, RelayRole};
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

impl Kernel {
    pub(crate) fn claim_profile(
        &mut self,
        pubkey: String,
        consumer_id: String,
        can_send: bool,
        force: bool,
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
        // ADR-0055 Rung 1: bump profile_claims_ver.
        // (diagnostics_inputs_ver is NOT co-bumped here — F5 derives it from the
        // relay_diagnostics payload fingerprint each emit, not per mutation site.)
        self.projection_rev_tracker.source_versions.bump_profile_claims();

        // F-TTL — a profile is a kind:0 replaceable identity. When the profile
        // is already cached, no cold fetch goes out; instead the TTL gate
        // decides whether a lazy re-verification REQ is due (`force == false`),
        // or unconditionally enqueues one (`force == true`, e.g. the user
        // opened this author's profile screen or pulled to refresh). Running
        // this only in the cached branch avoids double-fetching kind:0 on a
        // cold claim, which already issues its own request below.
        if self.profiles.contains_key(&pubkey) {
            if let Ok(pk) = nostr::PublicKey::from_hex(&pubkey) {
                self.claim_replaceable(0, pk.to_bytes(), None, force);
            }
            return Vec::new();
        }

        if self.profile_requests.requested.contains(&pubkey)
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
        // ADR-0055 Rung 1: bump profile_claims_ver.
        // (diagnostics_inputs_ver is NOT co-bumped here — F5 derives it from the
        // relay_diagnostics payload fingerprint each emit, not per mutation site.)
        self.projection_rev_tracker.source_versions.bump_profile_claims();
        self.log(format!(
            "release profile {} consumer {} ref {}",
            short_hex(pubkey),
            truncate(consumer_id, 80),
            remaining
        ));
        Vec::new()
    }

    /// Re-queue `pubkey` for kind:0 re-fetch after its NIP-65 mailbox just
    /// changed.
    ///
    /// **Why.** When a UI view claims a profile for an unknown pubkey the
    /// kernel batches a kind:0 fetch against the *indexer* lane (the only
    /// outbox set known cold-start — see `pending_profile_claim_requests`,
    /// lane 7 fallback `BootstrapSeed::IndexerOnly`). After kind:10002 lands
    /// for that pubkey the substrate `MailboxCache` knows the author's
    /// actual NIP-65 write set; any kind:0 we already cached came from the
    /// indexer only and may differ from what the author publishes on their
    /// own write relays. Moving the pubkey from `profile_requests.requested`
    /// (inflight or completed) back to `profile_requests.pending` lets the
    /// next `pending_view_requests` tick (which calls
    /// `pending_profile_claim_requests`) re-batch a fresh kind:0 REQ —
    /// this time routed through `route_outbox_subscription_relays` against
    /// the now-known write set.
    ///
    /// **Gating.** No-op when the pubkey is not in `requested` (nothing to
    /// refresh — never-claimed or first fetch still pending). Also no-op
    /// when the pubkey is already in `pending` (the upcoming tick will
    /// satisfy it). Does NOT clear `profiles` — the stale kind:0 stays as
    /// the rendered fallback until the fresh one arrives (D6: views never
    /// blank).
    ///
    /// Called from [`Kernel::on_mailbox_changed`] (production: substrate
    /// `Kind10002Parser` mutated the cache; test_support:
    /// `inject_replaceable_event`'s `10002 =>` arm substitutes the same
    /// effect).
    pub(in crate::kernel) fn refresh_profile_after_mailbox(&mut self, pubkey: &str) {
        if !self.profile_requests.requested.contains(pubkey) {
            return;
        }
        if self.profile_requests.pending.contains(pubkey) {
            return;
        }
        self.profile_requests.requested.remove(pubkey);
        self.profile_requests.pending.insert(pubkey.to_string());
        self.changed_since_emit = true;
        self.log(format!(
            "refresh profile after mailbox change {}",
            short_hex(pubkey)
        ));
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

        // Group authors by relay. The router resolves each author against
        // their NIP-65 read set (lane 1) — for kind:0 profile-claim discovery
        // an author published their kind:0 on their declared write relays,
        // but the router's `route_subscription` shape uses the read lane and
        // for NIP-65-known authors both lanes converge on the `both` marker
        // common to most kind:10002 entries. Cold-start authors fall through
        // to lane 7 with the **indexer-only** bootstrap seed (kind:0 probes
        // must never leak onto the shared content relay — the historical
        // `author_indexer_relays` contract).
        let mut by_relay: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        // Mark all as requested and remove from pending. We do this before
        // the router calls so the borrow checker is happy (the router
        // borrows `&self`, the cache update mutates `&mut self`).
        for author in &authors {
            self.profile_requests.pending.remove(author);
            self.profile_requests.requested.insert(author.clone());
        }
        self.profile_requests.req_seq = self.profile_requests.req_seq.saturating_add(1);
        let seq = self.profile_requests.req_seq;

        for (idx, author) in authors.iter().enumerate() {
            let interest_id = stable_hash64(("profile-claim-batch", seq, idx, author.as_str()));
            // Outbox-direction: kind:0 is published by the author to their
            // *write* relays. Router's `route_publish` shape returns the
            // author's NIP-65 write set (lane 1); cold-start falls back
            // to the indexer-only bootstrap seed via lane 7.
            let relays = self.route_outbox_subscription_relays(
                interest_id,
                author.as_str(),
                0,
                BootstrapSeed::IndexerOnly,
            );
            for relay_url in relays {
                by_relay.entry(relay_url).or_default().push(author.clone());
            }
        }

        // One batched REQ per relay with all authors in a single `authors` array.
        let mut requests = Vec::new();
        for (relay_url, mut relay_authors) in by_relay {
            // Stable author order per relay (plan-id / D8).
            crate::util::sort_dedup(&mut relay_authors);
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
        // T105: kind:0 is an outbox-direction discovery fetch — the author
        // published their kind:0 on their declared write relays. The
        // router's `route_publish` shape returns the author's NIP-65
        // write set (lane 1) for warm authors; cold-start falls back to
        // the indexer-only bootstrap seed via lane 7 (kind:0 probes
        // MUST NOT leak onto the shared content relay — historical
        // `author_indexer_relays` contract).
        let interest_id = stable_hash64(("profile-claim", pubkey.as_str(), seq));
        let relays = self.route_outbox_subscription_relays(
            interest_id,
            pubkey.as_str(),
            0,
            BootstrapSeed::IndexerOnly,
        );
        let mut requests = Vec::new();
        for relay_url in relays {
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
