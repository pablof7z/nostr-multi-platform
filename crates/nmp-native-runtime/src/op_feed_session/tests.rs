//! Tests for the OP-feed composition root's live, fail-closed active-follows
//! shape provider (ADR-0072 §8 6B, B1 logout-race fail-close).

use super::*;
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_nip02::{ActiveFollowSet, LatestKind3FollowSet};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nostr::{Event, EventBuilder, JsonUtil, Keys, SecretKey, Timestamp, ToBech32};

const TEST_FEED_KEY: &str = "test.op.feed.following";

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

fn keys_from_byte(byte: u8) -> Keys {
    let secret = SecretKey::from_slice(&[byte; 32]).expect("valid fixture secret");
    Keys::new(secret)
}

fn latest_kind3_reader() -> (LatestKind3FollowSet, Arc<dyn EventStore>) {
    let slot = nmp_core::slots::new_event_store_slot();
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    *slot.lock().expect("store slot") = Some(Arc::clone(&store));
    (LatestKind3FollowSet::new(slot), store)
}

fn insert_kind3(store: &Arc<dyn EventStore>, owner: &str, follows: &[&str]) {
    let tags = follows
        .iter()
        .map(|pk| vec!["p".to_string(), (*pk).to_string()])
        .collect();
    let raw = RawEvent {
        id: format!("{:0>64x}", follows.len() + 1),
        pubkey: owner.to_string(),
        created_at: 100,
        kind: nmp_core::kinds::KIND_CONTACT_LIST,
        tags,
        content: String::new(),
        sig: "22".repeat(64),
    };
    let _ = store.insert(
        VerifiedEvent::from_raw_unchecked(raw),
        &"wss://store.test/".to_string(),
        100_000,
    );
}

fn signed_note(keys: &Keys, content: &str, created_at: u64) -> Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
}

fn signed_contact_list(keys: &Keys, follows: &[&str], created_at: u64) -> Event {
    let tags = follows
        .iter()
        .map(|follow| nostr::Tag::parse(["p", *follow]).expect("valid p tag"));
    EventBuilder::new(nostr::Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind3")
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

fn visible_feed_ids(app: &crate::NmpApp, key: &str) -> Vec<String> {
    let Some(row) = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key && row.state != nmp_core::WireProjectionState::Cleared)
    else {
        return Vec::new();
    };
    nmp_note_feed::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NNFS payload decodes")
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
    let (reader, _store) = latest_kind3_reader();
    let follow_set = ActiveFollowSet::new(slot.clone(), reader);
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

/// The OP-feed pull shape hydrates from the active account's latest stored
/// kind:3 before any observer event arrives.
#[test]
fn provider_hydrates_shape_from_stored_kind3() {
    let alice = "a".repeat(64);
    let bob = "b".repeat(64);
    let slot: ActiveAccountSlot = Arc::new(Mutex::new(Some(alice.clone())));
    let (reader, store) = latest_kind3_reader();
    insert_kind3(&store, &alice, &[&bob]);
    let follow_set = ActiveFollowSet::new(slot.clone(), reader);
    let kinds: BTreeSet<u32> = [1u32, 6u32].into_iter().collect();

    let shape = live_active_follows_shape(&slot, &follow_set, &kinds)
        .expect("stored contacts should compile an active-follows shape");

    assert!(shape.authors.contains(&alice), "viewer is self-included");
    assert!(shape.authors.contains(&bob), "cached follow is included");
    assert_eq!(shape.authors.len(), 2);
    assert_eq!(shape.kinds, kinds);
}

/// Exercise the production active-follows composition path, not just the helper shape
/// provider. A stored active-account kind:3 hydrates the session-owned active
/// follows; a followed author's ingested note must then appear in the caller's
/// typed projection.
#[test]
fn active_follows_projection_renders_followed_note_from_stored_kind3() {
    let alice = keys_from_byte(11);
    let bob = keys_from_byte(12);
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let alice_kind3 = signed_contact_list(&alice, &[&bob_pk], 105);
    let bob_note = signed_note(&bob, "visible from cached contacts", 110);
    let bob_note_id = bob_note.id.to_hex();

    let app = crate::new_app();
    let projection = nmp_feed::ProjectionKey::app_owned(TEST_FEED_KEY).unwrap();
    let session =
        crate::open_active_follows_op_feed(&app, alice_pk.clone(), vec![1], projection.clone());
    assert!(
        session.handle.is_some(),
        "active-follows feed opens through the ordinary feed-session compiler"
    );
    assert!(
        app.registered_typed_projection_keys()
            .iter()
            .any(|key| key == projection.as_str()),
        "feed session registers the caller-owned typed projection"
    );

    let (tx, rx) = channel::<()>();
    app.set_update_listener(Some(Arc::new(move |_| {
        let _ = tx.send(());
    })));
    app.start_runtime(256, 8);

    let nsec = alice.secret_key().to_bech32().expect("nsec fixture");
    app.signin_nsec_for_test(nsec, true);
    wait_for(&rx, "active account selected", || {
        app.active_account_handle().lock().unwrap().as_deref() == Some(&alice_pk)
    });
    assert!(
        app.inject_signed_event_json_for_test(&alice_kind3.as_json()),
        "signed active-account kind3 verifies and enters ingest"
    );
    wait_for(&rx, "active follows hydrated from stored kind3", || {
        session.follow_set.follows().contains(&bob_pk)
    });
    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "identity-triggered observer registration must drain before note ingest"
    );

    assert!(
        app.inject_signed_event_json_for_test(&bob_note.as_json()),
        "signed followed-author note verifies and enters ingest"
    );
    wait_for(&rx, "visible active-follows feed row", || {
        visible_feed_ids(&app, projection.as_str()) == vec![bob_note_id.clone()]
    });

    app.set_update_listener(None);
    app.stop_runtime();
}

/// Empty host kinds also fail closed, regardless of slot/follows.
#[test]
fn provider_fails_closed_on_empty_kinds() {
    let alice = "a".repeat(64);
    let slot: ActiveAccountSlot = Arc::new(Mutex::new(Some(alice)));
    let (reader, _store) = latest_kind3_reader();
    let follow_set = ActiveFollowSet::new(slot.clone(), reader);
    let empty: BTreeSet<u32> = BTreeSet::new();
    assert!(live_active_follows_shape(&slot, &follow_set, &empty).is_none());
}
