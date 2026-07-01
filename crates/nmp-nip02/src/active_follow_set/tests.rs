//! Tests for [`ActiveFollowSet`] — the V-59 rung 4 follow-set producer.
//!
//! These mirror the sibling `FollowListProjection` test idioms: a hand-built
//! `ActiveAccountSlot` (`Arc<Mutex<Option<String>>>`) and hand-built kind:3
//! `KernelEvent`s. The producer is pure (no kernel harness needed) — the
//! `ObservedProjectionSink::on_kernel_event` entry point is driven directly, and
//! the account-change seam (`notify_account_changed`) is driven directly,
//! exactly as the composition root will at rung 6.

use super::*;
use nmp_core::substrate::{ContactsLookup, ContactsView, TestContactsCache};
use std::sync::atomic::{AtomicUsize, Ordering};

// Valid-looking 64-hex pubkeys, all distinct.
const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const DAVE: &str = "dd11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

fn slot(active: Option<&str>) -> ActiveAccountSlot {
    Arc::new(Mutex::new(active.map(str::to_string)))
}

fn empty_contacts() -> Arc<dyn ContactsLookup> {
    nmp_core::substrate::empty_contacts_lookup()
}

fn active_set(active: Option<&str>) -> Arc<ActiveFollowSet> {
    ActiveFollowSet::new(slot(active), empty_contacts())
}

fn upsert_contacts(cache: &TestContactsCache, owner: &str, follows: &[&str]) {
    cache.upsert_view(
        owner,
        ContactsView {
            event_id: owner.to_string(),
            created_at: 100,
            follows: follows.iter().map(|pk| (*pk).to_string()).collect(),
        },
    );
}

/// Build a kind:3 contact-list event authored by `author` with the given
/// `p`-tagged follows.
fn kind3(author: &str, p_tags: &[&str]) -> KernelEvent {
    let tags: Vec<Vec<String>> = p_tags
        .iter()
        .map(|pk| vec!["p".to_string(), (*pk).to_string()])
        .collect();
    KernelEvent {
        id: nmp_core::substrate::EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: KIND_CONTACT_LIST,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

// ─── self-inclusion ────────────────────────────────────────────────────────

#[test]
fn active_account_is_included_before_any_kind3() {
    // Self-inclusion mirrors `timeline_authors` seeding: the active account's
    // own pubkey is a member even before its kind:3 arrives.
    let set = active_set(Some(ALICE));
    assert_eq!(set.follows(), vec![ALICE.to_string()]);
    assert!(set.predicate()(ALICE));
}

#[test]
fn no_active_account_is_empty() {
    let set = active_set(None);
    assert!(set.follows().is_empty());
    assert!(!set.predicate()(ALICE));
}

// ─── kind:3 ingest updates follows() ─────────────────────────────────────────

#[test]
fn kind3_ingest_updates_follows() {
    let set = active_set(Some(ALICE));
    set.on_kernel_event(&kind3(ALICE, &[BOB, CAROL]));

    let follows = set.follows();
    // Bob, Carol, plus Alice herself (self-inclusion).
    assert!(follows.contains(&BOB.to_string()));
    assert!(follows.contains(&CAROL.to_string()));
    assert!(follows.contains(&ALICE.to_string()));
    assert_eq!(follows.len(), 3);
}

#[test]
fn newer_kind3_replaces_older_follow_list() {
    let set = active_set(Some(ALICE));
    set.on_kernel_event(&kind3(ALICE, &[BOB]));
    assert!(set.predicate()(BOB));

    // A replacement kind:3 that drops Bob and adds Carol.
    set.on_kernel_event(&kind3(ALICE, &[CAROL]));
    let pred = set.predicate();
    assert!(!pred(BOB), "dropped follow must no longer be in the set");
    assert!(pred(CAROL));
    assert!(pred(ALICE), "self-inclusion survives a replacement kind:3");
}

#[test]
fn non_active_author_kind3_is_ignored() {
    let set = active_set(Some(ALICE));
    // Carol (not the active account) publishes a kind:3 following Dave.
    set.on_kernel_event(&kind3(CAROL, &[DAVE]));
    assert!(
        !set.predicate()(DAVE),
        "a non-active author's kind:3 must not mutate the active set"
    );
    // Only Alice (self-inclusion) remains.
    assert_eq!(set.follows(), vec![ALICE.to_string()]);
}

#[test]
fn non_kind3_event_is_ignored() {
    let set = active_set(Some(ALICE));
    let mut ev = kind3(ALICE, &[BOB]);
    ev.kind = 1; // a kind:1 note — must not touch the follow set
    set.on_kernel_event(&ev);
    assert!(!set.predicate()(BOB));
    assert_eq!(set.follows(), vec![ALICE.to_string()]);
}

#[test]
fn cached_contacts_are_hydrated_before_kind3_ingest() {
    let cache = Arc::new(TestContactsCache::new());
    upsert_contacts(&cache, ALICE, &[BOB, CAROL]);
    let lookup: Arc<dyn ContactsLookup> = cache.clone();

    let set = ActiveFollowSet::new(slot(Some(ALICE)), lookup);

    let follows = set.follows();
    assert!(follows.contains(&ALICE.to_string()), "self is included");
    assert!(
        follows.contains(&BOB.to_string()),
        "cached follow is loaded"
    );
    assert!(
        follows.contains(&CAROL.to_string()),
        "cached follow is loaded"
    );
    assert_eq!(follows.len(), 3);
    assert!(set.predicate()(BOB));
}

// ─── predicate reflects the set live ─────────────────────────────────────────

#[test]
fn predicate_reflects_live_updates() {
    // The load-bearing property of the closure-only design (§3-D): a predicate
    // handed out BEFORE a follow is added still sees the follow afterwards,
    // because it captures a clone of the internal Arc<RwLock<…>>.
    let set = active_set(Some(ALICE));
    let pred = set.predicate();
    assert!(!pred(BOB), "Bob not followed yet");

    // Follow added after the predicate was handed out.
    set.on_kernel_event(&kind3(ALICE, &[BOB]));
    assert!(
        pred(BOB),
        "predicate handed out earlier must reflect the live set"
    );

    // And an unfollow handed out through the same predicate goes live too.
    set.on_kernel_event(&kind3(ALICE, &[CAROL]));
    assert!(
        !pred(BOB),
        "live unfollow visible through the old predicate"
    );
    assert!(pred(CAROL));
}

// ─── account switch rebuilds + fires on_change ───────────────────────────────

#[test]
fn account_switch_rebuilds_set_and_fires_on_change() {
    let slot = slot(Some(ALICE));
    let set = ActiveFollowSet::new(Arc::clone(&slot), empty_contacts());
    set.on_kernel_event(&kind3(ALICE, &[BOB, CAROL]));
    assert_eq!(set.follows().len(), 3);

    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    set.on_change(Box::new(move || {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // Switch to Dave: the kernel actor rewrites the slot, then the composition
    // root calls notify_account_changed.
    *slot.lock().unwrap() = Some(DAVE.to_string());
    set.notify_account_changed();

    // Alice's follows are gone; only Dave (self-inclusion) remains until
    // Dave's kind:3 arrives.
    let follows = set.follows();
    assert_eq!(follows, vec![DAVE.to_string()]);
    assert!(!set.predicate()(BOB), "prior account's follows are cleared");
    assert!(set.predicate()(DAVE), "new active account is self-included");
    assert_eq!(fired.load(Ordering::SeqCst), 1, "on_change fired on switch");

    // Dave's kind:3 now lands and repopulates.
    set.on_kernel_event(&kind3(DAVE, &[ALICE]));
    assert!(set.predicate()(ALICE));
    assert_eq!(
        fired.load(Ordering::SeqCst),
        2,
        "kind:3 ingest also fires on_change"
    );
}

#[test]
fn account_switch_hydrates_cached_contacts_for_new_account() {
    let cache = Arc::new(TestContactsCache::new());
    upsert_contacts(&cache, ALICE, &[BOB]);
    upsert_contacts(&cache, DAVE, &[CAROL]);
    let lookup: Arc<dyn ContactsLookup> = cache.clone();

    let slot = slot(Some(ALICE));
    let set = ActiveFollowSet::new(Arc::clone(&slot), lookup);
    assert!(set.predicate()(BOB), "initial cached follow is loaded");

    *slot.lock().unwrap() = Some(DAVE.to_string());
    set.notify_account_changed();

    let pred = set.predicate();
    assert!(!pred(ALICE), "prior account's self-entry is cleared");
    assert!(!pred(BOB), "prior account's cached follows are cleared");
    assert!(pred(DAVE), "new active account is self-included");
    assert!(pred(CAROL), "new account's cached follows are loaded");
}

// ─── logout clears the set ───────────────────────────────────────────────────

#[test]
fn logout_clears_set_and_fires_on_change() {
    let slot = slot(Some(ALICE));
    let set = ActiveFollowSet::new(Arc::clone(&slot), empty_contacts());
    set.on_kernel_event(&kind3(ALICE, &[BOB, CAROL]));
    assert_eq!(set.follows().len(), 3);

    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    set.on_change(Box::new(move || {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // Logout: the kernel actor clears the slot, then notify_account_changed.
    *slot.lock().unwrap() = None;
    set.notify_account_changed();

    assert!(set.follows().is_empty(), "logout clears the set entirely");
    let pred = set.predicate();
    assert!(
        !pred(ALICE),
        "predicate returns false for everyone after logout"
    );
    assert!(!pred(BOB));
    assert_eq!(fired.load(Ordering::SeqCst), 1, "on_change fired on logout");
}

// ─── on_change fires on each change ──────────────────────────────────────────

#[test]
fn on_change_fires_on_each_change() {
    let slot = slot(Some(ALICE));
    let set = ActiveFollowSet::new(Arc::clone(&slot), empty_contacts());

    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    set.on_change(Box::new(move || {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    // kind:3 update.
    set.on_kernel_event(&kind3(ALICE, &[BOB]));
    assert_eq!(fired.load(Ordering::SeqCst), 1);

    // Another kind:3 update.
    set.on_kernel_event(&kind3(ALICE, &[CAROL]));
    assert_eq!(fired.load(Ordering::SeqCst), 2);

    // Account switch.
    *slot.lock().unwrap() = Some(BOB.to_string());
    set.notify_account_changed();
    assert_eq!(fired.load(Ordering::SeqCst), 3);

    // Logout.
    *slot.lock().unwrap() = None;
    set.notify_account_changed();
    assert_eq!(fired.load(Ordering::SeqCst), 4);
}

#[test]
fn unchanged_kind3_echo_does_not_fire_on_change() {
    let set = active_set(Some(ALICE));

    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    set.on_change(Box::new(move || {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let contacts = kind3(ALICE, &[BOB, CAROL]);
    set.on_kernel_event(&contacts);
    assert_eq!(fired.load(Ordering::SeqCst), 1);

    set.on_kernel_event(&contacts);
    assert_eq!(
        fired.load(Ordering::SeqCst),
        1,
        "relay echo of identical contacts must not trigger a feed reset"
    );
}

#[test]
fn unchanged_account_notification_does_not_fire_on_change() {
    let cache = Arc::new(TestContactsCache::new());
    upsert_contacts(&cache, ALICE, &[BOB]);
    let lookup: Arc<dyn ContactsLookup> = cache.clone();

    let slot = slot(Some(ALICE));
    let set = ActiveFollowSet::new(Arc::clone(&slot), lookup);
    assert_eq!(set.follows(), vec![ALICE.to_string(), BOB.to_string()]);

    let fired = Arc::new(AtomicUsize::new(0));
    let fired_cb = Arc::clone(&fired);
    set.on_change(Box::new(move || {
        fired_cb.fetch_add(1, Ordering::SeqCst);
    }));

    set.notify_account_changed();
    assert_eq!(
        fired.load(Ordering::SeqCst),
        0,
        "same account and same cached follows are not a membership transition"
    );
}

#[test]
fn multiple_callbacks_all_fire() {
    let set = active_set(Some(ALICE));
    let a = Arc::new(AtomicUsize::new(0));
    let b = Arc::new(AtomicUsize::new(0));
    let a_cb = Arc::clone(&a);
    let b_cb = Arc::clone(&b);
    set.on_change(Box::new(move || {
        a_cb.fetch_add(1, Ordering::SeqCst);
    }));
    set.on_change(Box::new(move || {
        b_cb.fetch_add(1, Ordering::SeqCst);
    }));

    set.on_kernel_event(&kind3(ALICE, &[BOB]));
    assert_eq!(a.load(Ordering::SeqCst), 1);
    assert_eq!(b.load(Ordering::SeqCst), 1);
}

// ─── predicate is Send + Sync (engine consumes it across threads) ────────────

#[test]
fn predicate_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    let set = active_set(Some(ALICE));
    let pred = set.predicate();
    assert_send_sync(&pred);
}
