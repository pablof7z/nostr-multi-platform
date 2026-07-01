//! Tests for the OP-feed composition root's live, fail-closed active-follows
//! shape provider (ADR-0058 §8 6B, B1 logout-race fail-close).

use super::*;
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::substrate::{ContactsLookup, ContactsView, KernelEvent, TestContactsCache};
use nmp_core::ObservedProjectionSink;
use nmp_nip02::ActiveFollowSet;
use nostr::{Event, EventBuilder, JsonUtil, Keys, SecretKey, Timestamp, ToBech32};

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

fn keys_from_byte(byte: u8) -> Keys {
    let secret = SecretKey::from_slice(&[byte; 32]).expect("valid fixture secret");
    Keys::new(secret)
}

fn signed_note(keys: &Keys, content: &str, created_at: u64) -> Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
}

fn wait_for(rx: &Receiver<()>, label: &str, pred: impl Fn() -> bool) {
    if pred() {
        return;
    }
    loop {
        rx.recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
        if pred() {
            return;
        }
    }
}

fn visible_home_feed_ids(app: &crate::NmpApp) -> Vec<String> {
    let Some(row) = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| {
            row.key == nmp_note_feed::op_feed::OP_FEED_SNAPSHOT_KEY
                && row.state != nmp_core::WireProjectionState::Cleared
        })
    else {
        return Vec::new();
    };
    nmp_note_feed::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("nmp.feed.home NNFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
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

/// Regression for #2574 / #2500 product failure shape: exercise the production
/// default-home composition path, not just the helper shape provider. Cached
/// contacts hydrate the session-owned active follows before the active account's
/// kind:3 relays back; a followed author's ingested note must then appear in the
/// typed `nmp.feed.home` projection.
#[test]
fn default_home_projection_renders_followed_note_from_cached_contacts() {
    let alice = keys_from_byte(11);
    let bob = keys_from_byte(12);
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let bob_note = signed_note(&bob, "visible from cached contacts", 110);
    let bob_note_id = bob_note.id.to_hex();

    let app = crate::new_app();
    let cache = Arc::new(TestContactsCache::new());
    upsert_contacts(&cache, &alice_pk, &[&bob_pk]);
    let lookup: Arc<dyn ContactsLookup> = cache;
    assert_eq!(
        app.set_contacts_lookup(lookup),
        crate::NmpConfigStatus::Ok,
        "test contacts cache must be the app's canonical contacts lookup"
    );

    let defaults = crate::register_op_feed_defaults(&app, alice_pk.clone(), vec![1]);
    assert!(
        defaults.handle.is_some(),
        "default home feed opens through the ordinary feed-session compiler"
    );
    assert!(
        app.registered_typed_projection_keys()
            .iter()
            .any(|key| key == nmp_note_feed::op_feed::OP_FEED_SNAPSHOT_KEY),
        "default home feed registers the typed nmp.feed.home projection"
    );

    let (tx, rx) = channel::<()>();
    app.set_update_listener(Some(Arc::new(move |_| {
        let _ = tx.send(());
    })));
    app.start_runtime(256, 8);

    let nsec = alice.secret_key().to_bech32().expect("nsec fixture");
    app.signin_nsec_for_test(nsec, true);
    wait_for(&rx, "active follows hydrated from cached contacts", || {
        app.active_account_handle().lock().unwrap().as_deref() == Some(&alice_pk)
            && defaults.follow_set.follows().contains(&bob_pk)
    });
    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "identity-triggered observer registration must drain before note ingest"
    );

    assert!(
        app.inject_signed_event_json_for_test(&bob_note.as_json()),
        "signed followed-author note verifies and enters ingest"
    );
    wait_for(&rx, "visible default home feed row", || {
        visible_home_feed_ids(&app) == vec![bob_note_id.clone()]
    });

    app.set_update_listener(None);
    app.stop_runtime();
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
