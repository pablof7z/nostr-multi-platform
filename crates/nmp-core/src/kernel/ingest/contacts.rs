//! Kind:3 (contact list) ingest.

use super::super::{short_hex, BTreeSet, Kernel};
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::stable_hash::stable_hash64;
use crate::subs::{AccountId, CompileTrigger};
use std::collections::BTreeSet as BTreeSetInner;

/// Deterministic `InterestId` for the SINGLE multi-author follow-feed interest,
/// keyed by the compiled acquisition `kinds` set.
///
/// The follow-feed collapsed to ONE interest whose shape carries the whole
/// follow set in `authors` (#1497 amendment 5), so the id is keyed on `kinds`
/// only — the author set lives inside the interest's shape. Changing the follow
/// set REPLACES the slot's interest in place (same id, `push` = upsert);
/// switching the host kinds withdraws the old id and registers a fresh one.
///
/// Hashes `("follow-feed-authors", kinds_sorted_string)` so the same `kinds`
/// set always produces the same id across restarts, enabling stable
/// `withdraw` / `push` round-trips. `kinds_sorted_string` is the kinds rendered
/// in ascending order, joined by commas (e.g. `"1,6"`). A `BTreeSet` already
/// iterates in sorted order, so the rendering is deterministic.
fn follow_feed_interest_id(kinds: &BTreeSetInner<u32>) -> InterestId {
    let kinds_sorted_string = kinds
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    InterestId(stable_hash64((
        "follow-feed-authors",
        kinds_sorted_string.as_str(),
    )))
}

/// Build the SINGLE multi-author follow-feed `LogicalInterest` covering the
/// whole follow set (`authors`) under the compiled acquisition `kinds`
/// (`InterestLifecycle::Tailing`, `InterestScope::Global`).
///
/// `nmp-core` does not know which kinds belong to the host's app concept — the
/// `kinds` argument is supplied by the host through
/// `ActorCommand::OpenContactFeed { kinds }` (D0: the substrate
/// carries no app-specific social knowledge).
///
/// No `limit` (#1497 amendment 5): the feed already windows via `nmp-feed`, and
/// relays send what they choose, so a per-request limit is not load protection —
/// it existed only to force the per-author fan-out this collapse removes. The
/// `Tailing` lifecycle keeps the sub live past EOSE for new events.
fn follow_feed_interest(
    authors: BTreeSetInner<String>,
    kinds: &BTreeSetInner<u32>,
) -> LogicalInterest {
    LogicalInterest {
        id: follow_feed_interest_id(kinds),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: kinds.clone(),
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
    /// Withdraws the previously-registered follow-feed interest (tracked in
    /// `self.follow_feed_interest_ids`), then pushes ONE multi-author
    /// `LogicalInterest` whose shape covers the whole follow set + the active
    /// user into the lifecycle registry (#1497 amendment 5 — collapsed from the
    /// per-author fan-out). The `FollowListChanged` trigger is NOT enqueued
    /// here — callers are responsible for that (avoids duplicate triggers when
    /// this is called from a path that already enqueues).
    ///
    /// After this call the planner's next `drain_tick` will compile the new
    /// interest set and emit the correct REQ/CLOSE diff via `drain_lifecycle_tick`.
    pub(crate) fn sync_follow_feed_interests(&mut self, follows: &[String]) {
        // Withdraw the stale interest from the prior follow set / kinds.
        // Use drop_slot_by_key (legacy-key bridge) to remove any scope
        // the slot was registered under.
        {
            use crate::subs::InterestRegistry;
            let old_ids: Vec<InterestId> = self.follow_feed_interest_ids.iter().cloned().collect();
            for id in &old_ids {
                let key = InterestRegistry::legacy_key(id);
                self.lifecycle.registry_mut().drop_slot_by_key(key);
            }
        }
        self.follow_feed_interest_ids.clear();

        // D0: callers supply compiled acquisition kinds for the
        // contact-list-authors subscription via `ActorCommand::OpenContactFeed { kinds }`.
        // An empty `follow_feed_kinds` means the subscription is
        // NOT active — withdraw any existing interests (done above) and return
        // without registering. `nmp-core` never hardcodes a kind set here.
        let kinds = self.follow_feed_kinds.clone();
        if kinds.is_empty() {
            // `timeline_authors` is still cleared so a no-active-subscription
            // kernel does not gate-store events against a stale author set.
            self.timeline_authors = BTreeSet::new();
            return;
        }

        // Collect the whole author set: every follow + the active user (so the
        // user's own notes appear in their timeline). Distinct pubkeys collapse
        // into ONE multi-author interest — the planner's Case A routing fans the
        // shape per-author to each author's outbox relays at compile time
        // (`compiler/partition/case_a_authors.rs`).
        let mut authors: BTreeSetInner<String> = follows.iter().cloned().collect();
        if let Some(ref me) = self.active_account {
            authors.insert(me.clone());
        }

        // No authors (logout: empty follow set AND no active account) means the
        // subscription is NOT active — register nothing and clear the derived
        // author cache. An empty-authors interest would be a malformed REQ.
        if authors.is_empty() {
            self.timeline_authors = BTreeSet::new();
            return;
        }

        // Build the SINGLE multi-author follow-feed interest.
        let interest = follow_feed_interest(authors.clone(), &kinds);
        let interest_id = interest.id.clone();

        // Rebuild the `timeline_authors` derived cache from the new follow set
        // so `should_store_event` / `ingest_timeline_event` gate correctly.
        // `timeline_authors` is a denormalized read-cache over the M2 registry
        // (D4: the registry is the single source of truth; this is a projection).
        // Must be set BEFORE register_interest so enqueue_cache_serve's
        // `timeline_bound` flag is computed against the updated author set.
        self.timeline_authors = authors.into_iter().collect();

        // Derive the legacy identity for this interest (single synthetic owner,
        // planner-interest-id key) — matches what drop_slot_by_key used above.
        let identity = crate::subs::SubIdentity::from_legacy_interest(&interest);

        // ADR-0045 E1 — unified front-door: register + serve in one call.
        // The collapsed shape maps to ONE `StoreQuery::AuthorsKind`, so a
        // 300–500-follow cold start drains via one multi-author scan, not per
        // author (D1: first snapshot after install carries store data).
        //
        // ADR-0057 — the `pre_kind3_buffer` is DELETED; cache-serve here
        // surfaces prior stored events for any newly-added follows.
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::Replace,
            }],
            "follow-list-changed",
        );
        self.follow_feed_interest_ids.insert(interest_id);
    }

    /// ADR-0057 PR 3 — the kernel-owned follow-feed effects, driven by the
    /// ACTIVE account's kind:3 contacts transition.
    ///
    /// This is the PR 3 replacement for the deleted `ingest_contacts` arm. The
    /// kind:3 PARSING + cache write now lives in `nmp_nip01::Kind3Parser` (an
    /// `IngestParser` registered with the `EventIngestDispatcher`), which is
    /// structurally side-effect-free against kernel state. The kernel-owned
    /// planner/lifecycle effects an `IngestParser` cannot perform (they need
    /// `&mut self` + the active-account + the lifecycle registry) are driven
    /// HERE, on the kernel's own tick, by the contacts-transition SIGNAL that
    /// `project_accepted_event` detects (before/after snapshot of the
    /// capability-owned contacts cache for the author, exactly like the profile
    /// / mailbox / DM-relay transitions).
    ///
    /// `follows` is the freshly-parsed follow set the parser wrote into
    /// the cache (`contact_follows` — the SAME pure function the
    /// `nmp-nip02` follow-set observers use, so the router's `timeline_authors`,
    /// the follow predicate, and the `nmp.follow_list` snapshot can never
    /// diverge on which follows count). An empty `follows` is a CLEARED
    /// follow set (a kind:3 with no `p` tags), which correctly WITHDRAWS the
    /// prior follow-feed interests via `sync_follow_feed_interests(&[])`.
    ///
    /// Effects (all preserved EXACTLY from the old `ingest_contacts`):
    /// - **A11 `FollowListChanged` trigger** into the lifecycle inbox so the
    ///   subscription compiler recompiles on the next tick (D8: multiple kind:3
    ///   arrivals within one tick collapse to a single compile pass).
    /// - **M2 `LogicalInterest` (re)registration + `timeline_authors` rebuild +
    ///   cache-serve** via `sync_follow_feed_interests`.
    pub(in crate::kernel) fn on_active_contacts_changed(
        &mut self,
        author: &str,
        follows: Vec<String>,
        _created_at: u64,
    ) {
        self.log(format!(
            "contacts {} -> {} followees",
            short_hex(author),
            follows.len()
        ));

        // A11: fan a FollowListChanged trigger into the lifecycle inbox so the
        // subscription compiler recompiles on the next tick.
        self.lifecycle
            .enqueue_trigger(CompileTrigger::FollowListChanged {
                account_id: AccountId(author.to_string()),
                new_follows: follows.clone(),
            });

        // T140: register M2 LogicalInterests for the active account's follow set.
        // The FollowListChanged trigger above drives drain_lifecycle_tick to
        // recompile and emit the REQ/CLOSE diff on the next actor idle tick. This
        // is only ever reached for the active account (every caller gates on it),
        // so arbitrary peers' kind:3 events never pollute the registry (D4).
        //
        // Byte-estimate memo invalidation is the cache-WRITE site's job (the
        // `project_accepted_event` transition block / `prepopulate_contacts`),
        // not this effect body — so it fires for ANY author's contacts write, not
        // just the active-account transition that reaches here.
        self.sync_follow_feed_interests(&follows);
    }

    /// T140 — Re-register M2 follow-feed interests from the active account's
    /// current follow set in the capability-owned contacts cache
    /// (`Arc<dyn ContactsLookup>`).
    ///
    /// Called by `open_contact_feed()` (the `ActorCommand::OpenContactFeed`
    /// handler) so that switching screens back to the home feed re-confirms
    /// the M2 interest set is populated under the compiled acquisition
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
    /// Compiled-acquisition kinds setter for the contact-feed subscription.
    ///
    /// Callers use `ActorCommand::OpenContactFeed { kinds }` to supply the
    /// compiled acquisition kinds the active account's follow-set REQ should
    /// carry. D0: `nmp-core` does not know which primary kinds or wrapper
    /// policy belong to the host's app concept; the substrate just stores and
    /// threads the compiled set the caller supplies.
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
        // ADR-0057 PR 3 — read the active account's follow set from the
        // capability-owned contacts cache (`nmp_nip01::ContactsCache` behind
        // `Arc<dyn ContactsLookup>`) rather than the deleted kernel-owned
        // `seed_contacts` HashMap. `None` (no kind:3 cached yet) and
        // `Some(vec![])` (a cleared follow set) both correctly yield an empty
        // `follows`, which WITHDRAWS any stale follow-feed interests.
        let follows = self
            .contacts_lookup()
            .follows(&active_pk)
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
        // ADR-0057 — the `pre_kind3_buffer` is deleted; no per-identity parked
        // events to clear on a switch. The timeline read-cache itself is reset
        // elsewhere on identity change, and the new identity's follow set
        // re-projects its own events from the store via cache-serve below.
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
    fn follow_feed_interest_id_is_restart_stable() {
        let kinds = BTreeSetInner::from([1u32, 6u32]);
        // Restart-stable: the same kinds set hashes identically across calls.
        assert_eq!(
            follow_feed_interest_id(&kinds),
            follow_feed_interest_id(&kinds),
        );
        // Distinct kinds sets never collide, so switching the compiled
        // kinds withdraws the old id and registers a fresh one.
        assert_ne!(
            follow_feed_interest_id(&BTreeSetInner::from([1u32, 6u32])),
            follow_feed_interest_id(&BTreeSetInner::from([1u32])),
        );
    }

    #[test]
    fn follow_feed_interest_is_single_multi_author_no_limit() {
        let kinds = BTreeSetInner::from([1u32, 6u32]);
        let authors = BTreeSetInner::from([
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ]);
        let interest = follow_feed_interest(authors.clone(), &kinds);
        // ONE interest covers the whole author set (no per-author fan-out).
        assert_eq!(interest.shape.authors, authors);
        assert_eq!(interest.shape.kinds, kinds);
        // No `limit` (#1497 amendment 5) — the per-author backfill cap is gone.
        assert_eq!(interest.shape.limit, None);
    }
}
