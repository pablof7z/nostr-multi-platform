//! Tests for the OP-feed composition root's live, fail-closed active-follows
//! shape provider (ADR-0058 §8 6B, B1 logout-race fail-close).

use super::*;
use std::collections::BTreeSet;
use std::sync::Mutex;

use nmp_core::substrate::{ContactsLookup, ContactsView, KernelEvent, TestContactsCache};
use nmp_core::ObservedProjectionSink;
use nmp_nip02::ActiveFollowSet;

fn kind3(author: &str, follows: &[&str]) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = follows
        .iter()
        .map(|p| vec!["p".to_string(), (*p).to_string()])
        .collect();
    // A non-`p` tag to prove the follow-derivation ignores it.
    tags.push(vec!["client".to_string(), "test".to_string()]);
    KernelEvent {
        id: "c".repeat(64),
        author: author.to_string(),
        kind: 3,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
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

/// B1 logout race: the active-account slot can be cleared BEFORE the async
/// identity observer clears `ActiveFollowSet`, so `load_older` can observe
/// `slot == None` while `follows()` is still stale. The provider must read
/// the slot FIRST and fail closed (`None`) — never form a shape from the
/// stale follows (a stale-viewer pull).
#[test]
fn provider_fails_closed_when_slot_is_none_even_with_stale_follow_set() {
    let alice = "a".repeat(64);
    let bob = "b".repeat(64);
    let slot: ActiveAccountSlot = Arc::new(Mutex::new(Some(alice.clone())));
    let follow_set =
        ActiveFollowSet::new(slot.clone(), nmp_core::substrate::empty_contacts_lookup());
    // Populate a real, non-empty follow set for the active account.
    ObservedProjectionSink::on_kernel_event(&*follow_set, &kind3(&alice, &[&bob]));
    assert!(
        follow_set.follows().contains(&bob),
        "follow set seeded with a stale follow"
    );

    let kinds: BTreeSet<u32> = [1u32, 6u32].into_iter().collect();

    // While signed in, the provider yields a covered shape.
    assert!(
        live_active_follows_shape(&slot, &follow_set, &kinds).is_some(),
        "signed-in provider must yield a shape"
    );

    // Logout race: clear the SLOT but leave the follow set stale (the
    // identity observer has not run yet).
    *slot.lock().unwrap() = None;
    assert!(
        !follow_set.follows().is_empty(),
        "follow set is still stale (observer has not cleared it)"
    );

    // The provider must fail closed: slot read first ⇒ None ⇒ no shape, no
    // stale-viewer pull.
    assert!(
        live_active_follows_shape(&slot, &follow_set, &kinds).is_none(),
        "logout race must fail closed: None slot ⇒ no shape despite stale follows"
    );
}

/// Regression for #2500: account creation can prepopulate the shared contacts
/// cache before the active account's kind:3 relays back through ingest. The
/// OP-feed pull shape must use that cached follow set immediately, or seeded
/// home-feed rows are ingested but remain invisible.
#[test]
fn provider_hydrates_shape_from_cached_contacts_before_kind3_echo() {
    let alice = "a".repeat(64);
    let bob = "b".repeat(64);
    let slot: ActiveAccountSlot = Arc::new(Mutex::new(Some(alice.clone())));
    let cache = Arc::new(TestContactsCache::new());
    upsert_contacts(&cache, &alice, &[&bob]);
    let lookup: Arc<dyn ContactsLookup> = cache.clone();
    let follow_set = ActiveFollowSet::new(slot.clone(), lookup);
    let kinds: BTreeSet<u32> = [1u32, 6u32].into_iter().collect();

    let shape = live_active_follows_shape(&slot, &follow_set, &kinds)
        .expect("cached contacts should compile an active-follows shape");

    assert!(shape.authors.contains(&alice), "viewer is self-included");
    assert!(shape.authors.contains(&bob), "cached follow is included");
    assert_eq!(shape.authors.len(), 2);
    assert_eq!(shape.kinds, kinds);
}

/// Empty host kinds also fail closed, regardless of slot/follows.
#[test]
fn provider_fails_closed_on_empty_kinds() {
    let alice = "a".repeat(64);
    let slot: ActiveAccountSlot = Arc::new(Mutex::new(Some(alice)));
    let follow_set =
        ActiveFollowSet::new(slot.clone(), nmp_core::substrate::empty_contacts_lookup());
    let empty: BTreeSet<u32> = BTreeSet::new();
    assert!(live_active_follows_shape(&slot, &follow_set, &empty).is_none());
}
