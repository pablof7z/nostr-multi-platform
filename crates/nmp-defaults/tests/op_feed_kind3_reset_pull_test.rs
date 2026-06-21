//! Active-follows kind:3 replacement + pull replay regression for issue #1626.
//!
//! This is the composed proof that was missing from the split unit coverage:
//! an active-account kind:3 replacement updates the reactive follow predicate,
//! clears the native-facing typed home feed immediately, rewinds the pull cursor,
//! and lets the same open feed regrow from the Rust store under the new follow
//! set without the app/native shell doing anything.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_core::{decode_snapshot_typed_projections, encode_snapshot_frame, SnapshotEnvelope};
use nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY;
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const CAROL: &str = "cccc000000000000000000000000000000000000000000000000000000000003";
const RELAY: &str = "wss://test.relay/";

static SERIAL: Mutex<()> = Mutex::new(());

fn raw_note(id: &str, author: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "00".repeat(64),
    }
}

fn insert(store: &MemEventStore, raw: RawEvent) {
    store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &RELAY.to_string(),
            1_000,
        )
        .expect("insert must succeed");
}

fn kind3(id: &str, follows: &[&str]) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: ALICE.to_string(),
        created_at: 100,
        kind: nmp_core::kinds::KIND_CONTACT_LIST,
        tags: follows
            .iter()
            .map(|pubkey| vec!["p".to_string(), (*pubkey).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn set_active_account(app: &nmp_ffi::NmpApp, pubkey: &str) {
    *app.active_account_handle()
        .lock()
        .expect("active account slot") = Some(pubkey.to_string());
}

fn publish_store(app: &nmp_ffi::NmpApp, store: Arc<MemEventStore>) {
    let store: Arc<dyn EventStore> = store;
    *app.event_store_handle().lock().expect("event store slot") = Some(store);
}

fn typed_home_ids_from_snapshot_frame(app: &nmp_ffi::NmpApp) -> Vec<String> {
    let typed = app.run_typed_snapshot_projections();
    let frame = encode_snapshot_frame(&SnapshotEnvelope::default(), &typed);
    let rows = decode_snapshot_typed_projections(&frame).expect("snapshot frame sidecar decodes");
    let row = rows
        .iter()
        .find(|row| row.key == OP_FEED_SNAPSHOT_KEY)
        .expect("nmp.feed.home row is present in the typed sidecar");
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NOFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}

#[test]
fn kind3_replacement_clears_typed_home_and_replays_new_follow_rows_from_store() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_ffi::nmp_app_new();
    assert!(!app.is_null());
    let app_ref = unsafe { &*app };
    set_active_account(app_ref, ALICE);

    let store = Arc::new(MemEventStore::new());
    publish_store(app_ref, Arc::clone(&store));

    let defaults = nmp_defaults::register_op_feed_defaults(app_ref, ALICE.to_string(), vec![1]);

    let bob_note = "1".repeat(64);
    let carol_note = "2".repeat(64);
    insert(&store, raw_note(&bob_note, BOB, 1_100));
    insert(&store, raw_note(&carol_note, CAROL, 1_050));

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"3".repeat(64), &[BOB]));
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "initial pull should populate the active-account follow feed from the store"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        vec![bob_note.clone()],
        "the native-facing sidecar contains only the first kind:3 follow set"
    );

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"4".repeat(64), &[CAROL]));
    assert!(
        !defaults.follow_set.predicate()(BOB),
        "the replacement kind:3 removes the old follow from the reactive perspective"
    );
    assert!(
        defaults.follow_set.predicate()(CAROL),
        "the replacement kind:3 admits the new follow into the reactive perspective"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        Vec::<String>::new(),
        "kind:3 replacement clears the rendered typed feed immediately"
    );

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "kind:3 replacement must reset the seq cursor before replaying stored rows"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        vec![carol_note],
        "the same opened feed regrows from the store using the new follow set only"
    );

    nmp_ffi::nmp_app_free(app);
}
