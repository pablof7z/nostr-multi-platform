//! OP-feed suppression + pull pagination regressions.
//!
//! These tests prove that active-account suppression changes reset both the
//! visible OP-feed window and the pull cursor, so rows can disappear and
//! reappear from the Rust event store without native-shell intervention.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_core::{decode_snapshot_typed_projections, encode_snapshot_frame, SnapshotEnvelope};
use nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY;
use nmp_nip51::MuteListProjection;
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

fn raw_repost(id: &str, author: &str, created_at: u64, target_id: &str) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: nmp_nip18::KIND_REPOST,
        tags: vec![vec!["e".to_string(), target_id.to_string()]],
        content: String::new(),
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

fn mute_list(id: &str, muted_authors: &[&str], muted_events: &[&str]) -> KernelEvent {
    let mut tags = muted_authors
        .iter()
        .map(|pubkey| vec!["p".to_string(), (*pubkey).to_string()])
        .collect::<Vec<_>>();
    tags.extend(
        muted_events
            .iter()
            .map(|event_id| vec!["e".to_string(), (*event_id).to_string()]),
    );
    KernelEvent {
        id: id.to_string(),
        author: ALICE.to_string(),
        created_at: 200,
        kind: 10_000,
        tags,
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

fn current_home_ids(defaults: &nmp_defaults::OpFeedDefaults) -> Vec<String> {
    defaults
        .engine
        .snapshot_current_window()
        .cards
        .iter()
        .map(|card| card.card.id.clone())
        .collect()
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
fn mute_replacement_to_empty_replays_suppressed_rows_from_store() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_ffi::nmp_app_new();
    assert!(!app.is_null());
    let app_ref = unsafe { &*app };
    set_active_account(app_ref, ALICE);

    let store = Arc::new(MemEventStore::new());
    publish_store(app_ref, Arc::clone(&store));

    let mute = Arc::new(MuteListProjection::new(app_ref.active_account_handle()));
    let defaults = nmp_defaults::register_op_feed_defaults_with_mute(
        app_ref,
        ALICE.to_string(),
        vec![1],
        mute.clone(),
    );

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"3".repeat(64), &[BOB, CAROL]));
    let bob_note = "8".repeat(64);
    let carol_note = "9".repeat(64);
    insert(&store, raw_note(&bob_note, BOB, 1_100));
    insert(&store, raw_note(&carol_note, CAROL, 1_050));

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "first drain should populate the unmuted BOB/CAROL page"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![bob_note.clone(), carol_note.clone()],
        "precondition: both followed authors are visible before suppression changes"
    );

    mute.on_kernel_event(&mute_list(&"a".repeat(64), &[BOB], &[]));
    assert!(
        current_home_ids(&defaults).is_empty(),
        "mute replacement resets the visible window immediately"
    );
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "mute replacement must reset the seq cursor before replaying stored rows"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![carol_note.clone()],
        "the replay applies the new suppression policy and keeps only unmuted rows"
    );

    mute.on_kernel_event(&mute_list(&"b".repeat(64), &[], &[]));
    assert!(
        current_home_ids(&defaults).is_empty(),
        "unmuting is also a perspective change and clears the stale visible window"
    );
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "unmuting must reset the seq cursor so previously suppressed rows can re-enter"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![bob_note, carol_note],
        "the same opened feed replays from cache and restores rows after suppression is removed"
    );

    nmp_ffi::nmp_app_free(app);
}

#[test]
fn typed_home_sidecar_clears_and_regrows_after_mute_replacement() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_ffi::nmp_app_new();
    assert!(!app.is_null());
    let app_ref = unsafe { &*app };
    set_active_account(app_ref, ALICE);

    let store = Arc::new(MemEventStore::new());
    publish_store(app_ref, Arc::clone(&store));

    let mute = Arc::new(MuteListProjection::new(app_ref.active_account_handle()));
    let defaults = nmp_defaults::register_op_feed_defaults_with_mute(
        app_ref,
        ALICE.to_string(),
        vec![1],
        mute.clone(),
    );

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"8".repeat(64), &[BOB, CAROL]));
    let bob_note = "a".repeat(64);
    let carol_note = "b".repeat(64);
    insert(&store, raw_note(&bob_note, BOB, 1_100));
    insert(&store, raw_note(&carol_note, CAROL, 1_050));

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "initial pull should populate the home feed"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        vec![bob_note.clone(), carol_note.clone()],
        "native-facing SnapshotFrame sidecar carries the populated feed"
    );

    mute.on_kernel_event(&mute_list(&"9".repeat(64), &[BOB], &[]));
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        Vec::<String>::new(),
        "NIP-51 replacement clears nmp.feed.home in the typed sidecar immediately"
    );

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "load_older should replay from seq 0 after the same-shape mute change"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        vec![carol_note.clone()],
        "the decoded typed sidecar regrows from store with muted rows excluded"
    );

    mute.on_kernel_event(&mute_list(&"0".repeat(64), &[], &[]));
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        Vec::<String>::new(),
        "removing suppression also clears the stale typed feed before replay"
    );
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "unmute should replay from seq 0 so previously suppressed rows can return"
    );
    assert_eq!(
        typed_home_ids_from_snapshot_frame(app_ref),
        vec![bob_note, carol_note],
        "the typed sidecar regrows with both rows after suppression is removed"
    );

    nmp_ffi::nmp_app_free(app);
}

#[test]
fn muted_repost_targets_do_not_count_as_visible_pull_progress() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_ffi::nmp_app_new();
    assert!(!app.is_null());
    let app_ref = unsafe { &*app };
    set_active_account(app_ref, ALICE);

    let store = Arc::new(MemEventStore::new());
    publish_store(app_ref, Arc::clone(&store));

    let mute = Arc::new(MuteListProjection::new(app_ref.active_account_handle()));
    let defaults = nmp_defaults::register_op_feed_defaults_with_mute(
        app_ref,
        ALICE.to_string(),
        vec![1],
        mute.clone(),
    );

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"5".repeat(64), &[BOB]));

    let target_ids = (0..nmp_feed::DEFAULT_PULL_PAGE_SIZE)
        .map(|idx| format!("{:064x}", 0x1000 + idx))
        .collect::<Vec<_>>();
    for (idx, target_id) in target_ids.iter().enumerate() {
        insert(
            &store,
            raw_repost(
                &format!("{:064x}", 0x2000 + idx),
                BOB,
                2_000 + idx as u64,
                target_id,
            ),
        );
    }
    let direct_note = "6".repeat(64);
    insert(&store, raw_note(&direct_note, BOB, 1_000));

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "precondition: the first unmuted pull page surfaces repost placeholders"
    );
    assert_eq!(
        current_home_ids(&defaults).len(),
        nmp_feed::DEFAULT_PULL_PAGE_SIZE,
        "the first page is intentionally filled with repost-surfaced target rows"
    );

    let muted_event_refs = target_ids.iter().map(String::as_str).collect::<Vec<_>>();
    mute.on_kernel_event(&mute_list(&"7".repeat(64), &[], &muted_event_refs));
    assert!(
        current_home_ids(&defaults).is_empty(),
        "target-event suppression resets the existing repost-surfaced window"
    );

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "suppressed repost wrappers must not fill the rendered page; pull should continue to the visible direct note"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![direct_note],
        "after replay, muted target repost wrappers stay hidden and the followed author's later direct note appears"
    );

    nmp_ffi::nmp_app_free(app);
}

#[test]
fn event_id_mute_replacement_replays_suppressed_rows_from_store() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = nmp_ffi::nmp_app_new();
    assert!(!app.is_null());
    let app_ref = unsafe { &*app };
    set_active_account(app_ref, ALICE);

    let store = Arc::new(MemEventStore::new());
    publish_store(app_ref, Arc::clone(&store));

    let mute = Arc::new(MuteListProjection::new(app_ref.active_account_handle()));
    let defaults = nmp_defaults::register_op_feed_defaults_with_mute(
        app_ref,
        ALICE.to_string(),
        vec![1],
        mute.clone(),
    );

    defaults
        .follow_set
        .on_kernel_event(&kind3(&"4".repeat(64), &[BOB, CAROL]));
    let bob_note = "c".repeat(64);
    let carol_note = "d".repeat(64);
    insert(&store, raw_note(&bob_note, BOB, 1_100));
    insert(&store, raw_note(&carol_note, CAROL, 1_050));

    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "first drain should populate both followed authors"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![bob_note.clone(), carol_note.clone()],
        "precondition: both rows are visible before event-id suppression changes"
    );

    mute.on_kernel_event(&mute_list(&"e".repeat(64), &[], &[&bob_note]));
    assert!(
        current_home_ids(&defaults).is_empty(),
        "event-id mute replacement resets the visible window immediately"
    );
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "event-id mute replacement must reset the seq cursor before replay"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![carol_note.clone()],
        "replay applies e-tag suppression and keeps unrelated followed rows"
    );

    mute.on_kernel_event(&mute_list(&"f".repeat(64), &[], &[]));
    assert!(
        current_home_ids(&defaults).is_empty(),
        "removing the event-id mute also resets the stale visible window"
    );
    assert!(
        app_ref.load_older_feed(OP_FEED_SNAPSHOT_KEY),
        "unmuting by event id must replay stored rows from seq 0"
    );
    assert_eq!(
        current_home_ids(&defaults),
        vec![bob_note, carol_note],
        "the formerly event-id-muted row re-enters from the Rust store"
    );

    nmp_ffi::nmp_app_free(app);
}
