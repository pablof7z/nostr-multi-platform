//! Kind:3 (contact list) ingest.

use super::super::{is_hex_pubkey, short_hex, BTreeSet, Kernel, NostrEvent, TIMELINE_AUTHOR_LIMIT};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::stable_hash::stable_hash64;
use crate::subs::{AccountId, CompileTrigger};
use std::collections::BTreeSet as BTreeSetInner;

/// Deterministic `InterestId` for a follow-feed interest keyed by pubkey.
///
/// Hashes `("t140-follow-feed", pubkey)` so the same pubkey always produces the
/// same id across restarts, enabling stable `withdraw` / `push` round-trips.
fn follow_feed_interest_id(pubkey: &str) -> InterestId {
    InterestId(stable_hash64(("t140-follow-feed", pubkey)))
}

/// Per-author cap on the follow-feed REQ. Without this an `InterestShape` with
/// no bounds risks an unbounded backfill on the wire (codex finding #6).
const FOLLOW_FEED_LIMIT: u32 = 1000;

/// Build a `LogicalInterest` for a single follow-feed pubkey (kinds 1 and 6,
/// `InterestLifecycle::Tailing`, `InterestScope::Global`).
///
/// T140: carries `limit: Some(1000)`. The relay returns the newest 1000 events
/// and then tails — `Tailing` lifecycle keeps the sub live past EOSE for new events.
fn follow_feed_interest(pubkey: &str) -> LogicalInterest {
    let mut authors = BTreeSetInner::new();
    authors.insert(pubkey.to_string());
    let mut kinds = BTreeSetInner::new();
    kinds.insert(1u32);
    kinds.insert(6u32);
    LogicalInterest {
        id: follow_feed_interest_id(pubkey),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_feed_interest_id_is_restart_stable() {
        assert_eq!(
            follow_feed_interest_id(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            InterestId(0x7d88_17f3_d513_31d9)
        );
        assert_ne!(
            follow_feed_interest_id("alice"),
            follow_feed_interest_id("bob")
        );
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

        // Register one LogicalInterest per followed pubkey.
        for pubkey in follows {
            let interest = follow_feed_interest(pubkey);
            let id = interest.id.clone();
            self.lifecycle.registry_mut().push(interest);
            self.follow_feed_interest_ids.insert(id);
        }

        // Also register an interest for the active user themselves so their own
        // notes appear in the timeline.
        if let Some(ref me) = self.active_account {
            let interest = follow_feed_interest(me);
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
    }

    /// Ingest a kind:3 contact-list event into the local `seed_contacts` cache
    /// and fan a `FollowListChanged` (A11) trigger into the subscription
    /// lifecycle inbox.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced` (D4).
    /// Extracts "p"-tagged hex pubkeys, capping at `TIMELINE_AUTHOR_LIMIT`.
    ///
    /// T140: also calls `sync_follow_feed_interests` for the active account's
    /// kind:3 to register M2 `LogicalInterest`s into the lifecycle registry.
    /// The A11 trigger causes `drain_tick` (on the next tick boundary) to run
    /// a recompile and emit REQ frames for each followed author's NIP-65 write
    /// relays. The M1 hand-rolled `req()` path continues to run in parallel
    /// during the T140 verification window (Step A). Step C will retire M1 once
    /// M2 output is confirmed equivalent.
    pub(in crate::kernel) fn ingest_contacts(&mut self, event: NostrEvent) {
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
    }

    /// T140 — Re-register M2 follow-feed interests from the current
    /// `seed_contacts` of the active account.
    ///
    /// Called by `open_timeline()` (actor command) so that switching screens
    /// back to the timeline re-confirms the M2 interest set is populated.
    ///
    /// T140 (codex finding #4): empty / no-cached-follows must NOT no-op —
    /// that left the *previous* account's `follow_feed_interest_ids` and
    /// follow-derived `timeline_authors` live after an account switch or a
    /// missing kind:3. `sync_follow_feed_interests(&[])` withdraws every stale
    /// interest, clears the id set, and resets `timeline_authors` to empty;
    /// the trigger drives `drain_tick` to emit the CLOSE diff for the
    /// now-withdrawn subs. Calling it unconditionally is the correct CLEAR
    /// semantics.
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
