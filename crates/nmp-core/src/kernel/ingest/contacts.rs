//! Kind:3 (contact list) ingest.

use super::super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::subs::{AccountId, CompileTrigger};
use std::collections::BTreeSet as BTreeSetInner;

/// Deterministic `InterestId` for a follow-feed interest keyed by pubkey.
///
/// Hashes `("t140-follow-feed", pubkey)` so the same pubkey always produces the
/// same id across restarts, enabling stable `withdraw` / `push` round-trips.
fn follow_feed_interest_id(pubkey: &str) -> InterestId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "t140-follow-feed".hash(&mut h);
    pubkey.hash(&mut h);
    InterestId(h.finish())
}

/// Build a `LogicalInterest` for a single follow-feed pubkey (kinds 1 and 6,
/// `InterestLifecycle::Tailing`, `InterestScope::Global`).
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
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
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

        // Rebuild the `timeline_authors` derived cache from the new follow set
        // so `should_store_event` / `ingest_timeline_event` gate correctly.
        // `timeline_authors` is a denormalized read-cache over the M2 registry
        // (D4: the registry is the single source of truth; this is a projection).
        self.timeline_authors = follows.iter().cloned().collect();
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
    /// Idempotent: if the active account has no kind:3 cached yet, the registry
    /// stays empty until the first `ingest_contacts` fires.
    pub(crate) fn register_follow_feed_for_active_account(&mut self) {
        let Some(active_pk) = self.active_account.clone() else {
            return;
        };
        let follows = self
            .seed_contacts
            .get(&active_pk)
            .cloned()
            .unwrap_or_default();
        if !follows.is_empty() {
            self.sync_follow_feed_interests(&follows);
            // Enqueue a trigger so drain_tick recompiles on the next idle tick.
            use crate::subs::CompileTrigger;
            self.lifecycle
                .enqueue_trigger(CompileTrigger::FollowListChanged {
                    account_id: crate::subs::AccountId(active_pk),
                    new_follows: follows,
                });
        }
    }
}
