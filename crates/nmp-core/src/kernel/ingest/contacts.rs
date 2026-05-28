//! Kind:3 (contact list) ingest.

use super::super::{is_hex_pubkey, short_hex, BTreeSet, Kernel, NostrEvent, TIMELINE_AUTHOR_LIMIT};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::stable_hash::stable_hash64;
use crate::subs::{AccountId, CompileTrigger};
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
/// `ActorCommand::OpenContactListSubscription { kinds }` (D0: the substrate
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
        // subscription should REQ via `ActorCommand::OpenContactListSubscription
        // { kinds }`. An empty `follow_feed_kinds` means the subscription is
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
    /// Called by `open_contact_list_sub()` (the
    /// `ActorCommand::OpenContactListSubscription` handler) so that switching
    /// screens back to the timeline re-confirms the M2 interest set is populated
    /// under the host-declared `follow_feed_kinds`.
    ///
    /// T140 (codex finding #4): empty / no-cached-follows must NOT no-op —
    /// that left the *previous* account's `follow_feed_interest_ids` and
    /// follow-derived `timeline_authors` live after an account switch or a
    /// missing kind:3. `sync_follow_feed_interests(&[])` withdraws every stale
    /// interest, clears the id set, and resets `timeline_authors` to empty;
    /// the trigger drives `drain_tick` to emit the CLOSE diff for the
    /// now-withdrawn subs. Calling it unconditionally is the correct CLEAR
    /// semantics.
    /// Host-declared kinds setter for the contact-list-authors subscription.
    ///
    /// The host (e.g. Chirp) calls this via
    /// `ActorCommand::OpenContactListSubscription { kinds }` to declare which
    /// event kinds the active account's follow-set REQ should carry. D0:
    /// `nmp-core` does not know which kinds belong to the host's app concept
    /// (Chirp's social timeline is {1, 6}; another app might want {30023}); the
    /// substrate just stores and threads the set the host supplies.
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
