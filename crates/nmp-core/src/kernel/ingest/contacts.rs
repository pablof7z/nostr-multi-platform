//! Kind:3 (contact list) ingest.

use super::super::{short_hex, BTreeSet, Kernel, NostrEvent};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::RelayRole;
use crate::stable_hash::stable_hash64;
use crate::subs::{AccountId, CompileTrigger};
use crate::substrate::{BoundedMessageMap, MAX_PROJECTION_MESSAGES};
use std::collections::BTreeSet as BTreeSetInner;

/// Deterministic `InterestId` for a contact-list-authors interest keyed by
/// pubkey and the host-declared `kinds` set.
///
/// Hashes `("contact-list-authors", pubkey, kinds_sorted_string)` so the same
/// `(pubkey, kinds)` pair always produces the same id across restarts, enabling
/// stable `withdraw` / `push` round-trips. The `kinds` component means two
/// registrations of the same pubkey under different kind sets do NOT collide —
/// switching kinds withdraws the old interest id and pushes a fresh one.
///
/// `kinds_sorted_string` is the kinds rendered in ascending order, joined by
/// commas (e.g. `"1,6"`). A `BTreeSet` already iterates in sorted order, so the
/// rendering is deterministic.
fn contact_list_authors_interest_id(pubkey: &str, kinds: &BTreeSetInner<u32>) -> InterestId {
    let kinds_sorted_string = kinds
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    InterestId(stable_hash64((
        "contact-list-authors",
        pubkey,
        kinds_sorted_string.as_str(),
    )))
}

/// Per-author cap on the contact-list-authors REQ. Without this an
/// `InterestShape` with no bounds risks an unbounded backfill on the wire
/// (codex finding #6).
const FOLLOW_FEED_LIMIT: u32 = 1000;

/// Build a `LogicalInterest` for a single contact-list-author pubkey using the
/// host-declared `kinds` set (`InterestLifecycle::Tailing`,
/// `InterestScope::Global`).
///
/// `nmp-core` does not know which kinds belong to the host's app concept — the
/// `kinds` argument is supplied by the host through
/// `ActorCommand::OpenContactFeed { kinds }` (D0: the substrate
/// carries no app-specific social knowledge).
///
/// Carries `limit: Some(1000)`. The relay returns the newest 1000 events and
/// then tails — `Tailing` lifecycle keeps the sub live past EOSE for new events.
fn follow_feed_interest(pubkey: &str, kinds: &BTreeSetInner<u32>) -> LogicalInterest {
    let mut authors = BTreeSetInner::new();
    authors.insert(pubkey.to_string());
    LogicalInterest {
        id: contact_list_authors_interest_id(pubkey, kinds),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: kinds.clone(),
            limit: Some(FOLLOW_FEED_LIMIT),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        // Follow-feed timeline interests ride NIP-65 outbox routing; T134
        // invariant: never divert tailing follow feeds to the indexer.
        is_indexer_discovery: false,
    }
}

impl Kernel {
    /// T140 — Register (or replace) M2 `LogicalInterest`s for the active
    /// account's follow set.
    ///
    /// Withdraws any previously-registered follow-feed interests (tracked in
    /// `self.follow_feed_interest_ids`), then pushes one new `LogicalInterest`
    /// per pubkey in `follows` into the lifecycle registry. The `FollowListChanged`
    /// trigger is NOT enqueued here — callers are responsible for that (avoids
    /// duplicate triggers when this is called from a path that already enqueues).
    ///
    /// After this call the planner's next `drain_tick` will compile the new
    /// interest set and emit the correct REQ/CLOSE diff via `drain_lifecycle_tick`.
    pub(crate) fn sync_follow_feed_interests(&mut self, follows: &[String]) {
        // Withdraw stale interests from the prior follow set.
        let old_ids: Vec<InterestId> = self.follow_feed_interest_ids.iter().cloned().collect();
        for id in &old_ids {
            self.lifecycle.registry_mut().withdraw(id);
        }
        self.follow_feed_interest_ids.clear();

        // D0: the host declares which kinds the contact-list-authors
        // subscription should REQ via `ActorCommand::OpenContactFeed { kinds }`. An empty `follow_feed_kinds` means the subscription is
        // NOT active — withdraw any existing interests (done above) and return
        // without registering. `nmp-core` never hardcodes a kind set here.
        let kinds = self.follow_feed_kinds.clone();
        if kinds.is_empty() {
            // `timeline_authors` is still cleared so a no-active-subscription
            // kernel does not gate-store events against a stale author set.
            self.timeline_authors = BTreeSet::new();
            return;
        }

        // Register one LogicalInterest per followed pubkey.
        for pubkey in follows {
            let interest = follow_feed_interest(pubkey, &kinds);
            let id = interest.id.clone();
            self.lifecycle.registry_mut().push(interest);
            self.follow_feed_interest_ids.insert(id);
        }

        // Also register an interest for the active user themselves so their own
        // notes appear in the timeline.
        if let Some(ref me) = self.active_account {
            let interest = follow_feed_interest(me, &kinds);
            let id = interest.id.clone();
            self.lifecycle.registry_mut().push(interest);
            self.follow_feed_interest_ids.insert(id);
        }

        // Rebuild the `timeline_authors` derived cache from the new follow set
        // so `should_store_event` / `ingest_timeline_event` gate correctly.
        // `timeline_authors` is a denormalized read-cache over the M2 registry
        // (D4: the registry is the single source of truth; this is a projection).
        let mut authors: BTreeSet<String> = follows.iter().cloned().collect();
        if let Some(ref me) = self.active_account {
            authors.insert(me.clone());
        }
        self.timeline_authors = authors;

        // V-59 rung 1 (Q7) — the follow set just grew (or was rebuilt). Replay
        // any parked host-declared follow-feed events whose author is now in
        // `timeline_authors`; drop the rest. Must run AFTER `timeline_authors`
        // is rebuilt above so the re-ingest's `should_store_event` gate passes.
        self.flush_pre_kind3_buffer();

        // ADR-0045 E1 — store-cache serve for follow-feed interests.
        //
        // `sync_follow_feed_interests` uses the legacy `push` path (not
        // `open_interest_sub`) so the cache-serve hook in `open_interest_sub`
        // does not fire here. Enqueue a serve for every newly-registered
        // follow-feed interest, then drain ONE aggregate-budget chunk
        // synchronously so the next snapshot carries store data (D1). The
        // remainder continues on the actor tick (§5 chunked continuation) —
        // a 300–500-follow cold start never bursts unbounded synchronous
        // work on the actor thread.
        //
        // We reconstruct each interest's shape directly from `(pubkey, kinds)`
        // rather than looking it up via the registry to keep this O(n) instead
        // of O(n²). The `follow_feed_interest` constructor is deterministic.
        //
        // Route every author through the SHARED enqueue helper
        // (`enqueue_interest_cache_serve_deferred`) so the completion-key
        // derivation is the one centralised recipe — no hand-copied
        // `legacy_key` → `completion_key_for_interest` block to drift (PR #1237
        // review F3). `_deferred` enqueues WITHOUT draining; we drain ONCE after
        // the whole batch so a 300–500-follow cold start runs one synchronous
        // chunk, not one per author.
        {
            use crate::subs::InterestRegistry;
            // Collect the pubkey list: follows + active user.
            let mut cache_serve_authors: Vec<String> = follows.to_vec();
            if let Some(ref me) = self.active_account {
                cache_serve_authors.push(me.clone());
            }
            for pubkey in cache_serve_authors {
                let interest = follow_feed_interest(&pubkey, &kinds);
                let sub_key = InterestRegistry::legacy_key(&interest.id);
                self.enqueue_interest_cache_serve_deferred(&sub_key, &interest.shape);
            }
            self.run_cache_serve_step();
        }
    }

    /// Replay parked pre-kind:3 events whose author is now in
    /// `timeline_authors`, dropping the rest. (V-59 rung 1, Q7.)
    ///
    /// Takes the buffer out (replacing it with a fresh bounded map) so the
    /// replay's `&mut self` re-borrow does not alias the buffer iteration.
    /// Each matching entry is re-fed through `ingest_timeline_event` with its
    /// recorded provenance relay; a now-followed author makes the
    /// `should_store_event` gate's first clause (`timeline_authors.contains`)
    /// pass, so the event is finally stored. Non-matching entries are dropped
    /// — an author who is still not followed has no home-feed claim, and the
    /// next follow-set change will re-park fresh arrivals anyway.
    ///
    /// `sub_id` is empty on replay: the author now being in `timeline_authors`
    /// satisfies `should_store_event` regardless of sub_id, and an empty id
    /// cannot collide with the prefix-matched sub schemes
    /// (`seed-timeline-`, `thread-…`, …). `RelayRole::Content` is inert —
    /// `ingest_timeline_event` ignores `_role`.
    fn flush_pre_kind3_buffer(&mut self) {
        if self.pre_kind3_buffer.is_empty() {
            return;
        }
        let parked = std::mem::replace(
            &mut self.pre_kind3_buffer,
            BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
        );
        for (_id, (event, provenance)) in parked.iter() {
            if self.timeline_authors.contains(&event.pubkey) {
                let _ =
                    self.ingest_timeline_event(RelayRole::Content, provenance, "", event.clone());
            }
            // else: author still not followed → drop (do not re-park).
        }
    }

    /// Ingest a kind:3 contact-list event into the local `seed_contacts` cache
    /// and fan a `FollowListChanged` (A11) trigger into the subscription
    /// lifecycle inbox.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced` (D4).
    /// Extracts "p"-tagged hex pubkeys, capping at `TIMELINE_AUTHOR_LIMIT`, via
    /// the shared `crate::tags::capped_contact_follows` — the SAME pure function
    /// the `nmp-nip02` follow-set observers call, so the router's
    /// `timeline_authors` and the NIP-02 read models never diverge on the cap.
    ///
    /// T140: also calls `sync_follow_feed_interests` for the active account's
    /// kind:3 to register M2 `LogicalInterest`s into the lifecycle registry.
    /// The A11 trigger causes `drain_tick` (on the next tick boundary) to run
    /// a recompile and emit REQ frames for each followed author's NIP-65 write
    /// relays. The M1 hand-rolled `req()` path continues to run in parallel
    /// during the T140 verification window (Step A). Step C will retire M1 once
    /// M2 output is confirmed equivalent.
    pub(in crate::kernel) fn ingest_contacts(&mut self, event: NostrEvent) {
        // Single source of truth for the follow cap: the same pure function the
        // `nmp-nip02` `ActiveFollowSet` / `FollowListProjection` observers call,
        // so the router's `timeline_authors`, the follow predicate, and the
        // `nmp.follow_list` snapshot can never diverge on which 500 follows
        // count (first-500-valid-hex-p-tags in document order).
        let follows = crate::tags::capped_contact_follows(&event.tags);

        self.log(format!(
            "contacts {} -> {} followees",
            short_hex(&event.pubkey),
            follows.len()
        ));

        // A11: fan a FollowListChanged trigger into the lifecycle inbox so the
        // subscription compiler recompiles on the next tick. Per D8, multiple
        // kind:3 arrivals within one tick collapse to a single compile pass.
        self.lifecycle
            .enqueue_trigger(CompileTrigger::FollowListChanged {
                account_id: AccountId(event.pubkey.clone()),
                new_follows: follows.clone(),
            });

        // T140: register M2 LogicalInterests for the active account's follow set.
        // The FollowListChanged trigger above drives drain_lifecycle_tick to recompile
        // and emit the REQ/CLOSE diff on the next actor idle tick. Active-account
        // gated so arbitrary peers' kind:3 events don't pollute the registry (D4).
        let is_active = self.active_account.as_deref() == Some(event.pubkey.as_str());
        if is_active {
            self.sync_follow_feed_interests(&follows);
        }

        self.seed_contacts.insert(event.pubkey, follows);
        self.cached_estimated_store_bytes.set(None);
    }

    /// T140 — Re-register M2 follow-feed interests from the current
    /// `seed_contacts` of the active account.
    ///
    /// Called by `open_contact_feed()` (the `ActorCommand::OpenContactFeed`
    /// handler) so that switching screens back to the home feed re-confirms
    /// the M2 interest set is populated under the host-declared
    /// `follow_feed_kinds`.
    ///
    /// T140 (codex finding #4): empty / no-cached-follows must NOT no-op —
    /// that left the *previous* account's `follow_feed_interest_ids` and
    /// follow-derived `timeline_authors` live after an account switch or a
    /// missing kind:3. `sync_follow_feed_interests(&[])` withdraws every stale
    /// interest, clears the id set, and resets `timeline_authors` to empty;
    /// the trigger drives `drain_tick` to emit the CLOSE diff for the
    /// now-withdrawn subs. Calling it unconditionally is the correct CLEAR
    /// semantics.
    /// Host-declared kinds setter for the contact-feed subscription.
    ///
    /// The host (e.g. Chirp) calls this via
    /// `ActorCommand::OpenContactFeed { kinds }` to declare which event kinds
    /// the active account's follow-set REQ should carry. D0: `nmp-core` does
    /// not know which kinds belong to the host's app concept (Chirp's home
    /// feed is {1, 6}; a long-form app might want {30023}); the substrate just
    /// stores and threads the set the host supplies.
    ///
    /// Setting the kinds and then calling
    /// `register_follow_feed_for_active_account` re-registers the active
    /// account's follow-feed interests under the new kind set. An empty `kinds`
    /// set deactivates the subscription (withdraws every follow-feed interest).
    pub(crate) fn set_follow_feed_kinds(&mut self, kinds: BTreeSet<u32>) {
        self.follow_feed_kinds = kinds;
        self.register_follow_feed_for_active_account();
    }

    pub(crate) fn register_follow_feed_for_active_account(&mut self) {
        let Some(active_pk) = self.active_account.clone() else {
            return;
        };
        let follows = self
            .seed_contacts
            .get(&active_pk)
            .cloned()
            .unwrap_or_default();
        // Unconditional: empty `follows` CLEARs stale state (no-op was the bug).
        self.sync_follow_feed_interests(&follows);
        // Enqueue a trigger so drain_tick recompiles on the next idle tick —
        // including the empty case, where the recompile emits the CLOSE diff
        // that tears down the prior account's follow-feed subs.
        use crate::subs::CompileTrigger;
        self.lifecycle
            .enqueue_trigger(CompileTrigger::FollowListChanged {
                account_id: crate::subs::AccountId(active_pk),
                new_follows: follows,
            });
    }

    /// T168 — reconcile the M2 follow-feed after an identity change
    /// (logout / remove / switch). Call AFTER `sync_kernel` has updated
    /// `active_account` to the NEW active (or `None` on logout).
    ///
    /// - `active_account = Some(new)`: delegate to
    ///   `register_follow_feed_for_active_account()` — it withdraws the prior
    ///   account's interests and installs the new account's follows (or clears
    ///   to empty when the new account has no cached kind:3), and enqueues the
    ///   recompile trigger.
    /// - `active_account = None` (logged out of the last account):
    ///   `register_follow_feed_for_active_account()` early-returns, so do the
    ///   CLEAR here — `sync_follow_feed_interests(&[])` withdraws every stale
    ///   interest, resets `timeline_authors` to empty, and we enqueue a
    ///   `FollowListChanged{ new_follows: [] }` so `drain_tick` emits the CLOSE
    ///   diff that tears down the prior account's follow-feed subs (privacy
    ///   leak + stale-feed fix).
    pub(crate) fn reconcile_follow_feed_after_identity_change(&mut self) {
        // V-59 rung 1 (Q7) — drop the prior identity's parked pre-kind:3
        // events BEFORE the follow-set sync below. Replaying them against the
        // NEW account's follow set could surface the previous account's
        // unfollowed-author notes (privacy + stale-feed leak), and the new
        // account will re-park fresh arrivals anyway. Clearing here, ahead of
        // `sync_follow_feed_interests` (which flushes the buffer), guarantees
        // the flush sees an empty buffer on the switch.
        self.pre_kind3_buffer = BoundedMessageMap::new(MAX_PROJECTION_MESSAGES);
        // ADR-0045 E1 — clear the served-interest completion set AND the
        // pending serve queue so the new identity's interests get a fresh
        // store-cache serve and the prior identity's queued serves stop.
        // Must precede `sync_follow_feed_interests` so that the serve that
        // runs there starts from a clean slate.
        self.clear_served_interest_shapes();
        if self.active_account.clone().is_some() {
            self.register_follow_feed_for_active_account()
        } else {
            self.sync_follow_feed_interests(&[]);
            use crate::subs::CompileTrigger;
            self.lifecycle
                .enqueue_trigger(CompileTrigger::FollowListChanged {
                    account_id: crate::subs::AccountId(String::new()),
                    new_follows: Vec::new(),
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_list_authors_interest_id_is_restart_stable() {
        let kinds = BTreeSetInner::from([1u32, 6u32]);
        // Restart-stable: the same (pubkey, kinds) pair hashes identically
        // across calls.
        assert_eq!(
            contact_list_authors_interest_id(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &kinds,
            ),
            contact_list_authors_interest_id(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &kinds,
            ),
        );
        // Distinct pubkeys never collide.
        assert_ne!(
            contact_list_authors_interest_id("alice", &kinds),
            contact_list_authors_interest_id("bob", &kinds),
        );
        // Distinct kinds sets for the same pubkey never collide, so switching
        // the host-declared kinds withdraws the old id and pushes a fresh one.
        assert_ne!(
            contact_list_authors_interest_id("alice", &BTreeSetInner::from([1u32, 6u32])),
            contact_list_authors_interest_id("alice", &BTreeSetInner::from([1u32])),
        );
    }
}
