//! Tests for the Chirp per-open author / thread flat-feed registration
//! (ADR-0042 §5.1, ADR-0058 §8 6B viewport grow wiring).

use super::*;
use std::ffi::{CStr, CString};

use nmp_core::store::{MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::WireProjectionState;
use nmp_ffi::{nmp_app_free, nmp_app_new};

#[test]
fn keys_are_namespaced_per_consumer() {
    assert_eq!(author_feed_key("abc"), "nmp.feed.author.abc");
    assert_eq!(thread_feed_key("def"), "nmp.feed.thread.def");
    assert_eq!(author_consumer("abc"), "author-abc");
    assert_eq!(thread_consumer("def"), "thread-def");
}

#[test]
fn filter_json_carries_the_feed_kinds_and_dimension() {
    // The {1,6} policy in the filter MUST match FEED_KINDS (the predicate
    // source), or the kernel admits events the feed drops / vice versa.
    assert_eq!(FEED_KINDS, [1, 6]);
    assert_eq!(
        feed_filter_json("authors", "abc"),
        r#"{"kinds":[1,6],"authors":["abc"]}"#
    );
    // `r##"…"##` — the inner `"#e"` contains a `"#` sequence that would
    // terminate a single-hash raw string early.
    assert_eq!(
        feed_filter_json("#e", "root1"),
        r##"{"kinds":[1,6],"#e":["root1"]}"##
    );
}

#[test]
fn feed_filter_json_parses_as_a_valid_interest_shape() {
    // Guards the hand-built JSON against the kernel-side parser the open
    // path feeds it into — a malformed filter would be silently rejected.
    for json in [
        feed_filter_json("authors", "abc"),
        feed_filter_json("#e", "root1"),
    ] {
        assert!(
            nmp_core::planner::InterestShape::from_filter_json(&json).is_some(),
            "filter must parse: {json}"
        );
    }
}

#[test]
fn author_feed_open_seeds_cached_kind1_and_close_removes_projection() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let store = Arc::new(MemEventStore::new());
    let pubkey = "11".repeat(32);
    insert_raw(
        &store,
        RawEvent {
            id: "a1".repeat(32),
            pubkey: pubkey.clone(),
            created_at: 10,
            kind: 1,
            tags: vec![],
            content: "older".into(),
            sig: "a".repeat(128),
        },
    );
    insert_raw(
        &store,
        RawEvent {
            id: "a2".repeat(32),
            pubkey: pubkey.clone(),
            created_at: 20,
            kind: 1,
            tags: vec![],
            content: "newer".into(),
            sig: "a".repeat(128),
        },
    );
    install_store(app, store);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let ids = read_typed_card_ids(app, &author_feed_key(&pubkey))
        .expect("author feed projection present after open");
    assert_eq!(ids, vec!["a2".repeat(32), "a1".repeat(32)]);

    nmp_app_chirp_close_author_feed(app, pubkey_c.as_ptr());
    let gone = typed_projection_is_gone(app, &author_feed_key(&pubkey));
    assert!(gone, "author feed projection must be gone after close");
    nmp_app_free(app);
}

#[test]
fn thread_feed_open_seeds_cached_root_and_replies() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let store = Arc::new(MemEventStore::new());
    let root_id = "b1".repeat(32);
    insert_raw(
        &store,
        RawEvent {
            id: root_id.clone(),
            pubkey: "22".repeat(32),
            created_at: 10,
            kind: 1,
            tags: vec![],
            content: "root".into(),
            sig: "a".repeat(128),
        },
    );
    insert_raw(
        &store,
        RawEvent {
            id: "b2".repeat(32),
            pubkey: "33".repeat(32),
            created_at: 20,
            kind: 1,
            tags: vec![vec!["e".into(), root_id.clone()]],
            content: "reply".into(),
            sig: "a".repeat(128),
        },
    );
    install_store(app, store);

    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
    let ids = read_typed_card_ids(app, &thread_feed_key(&root_id))
        .expect("thread feed projection present after open");
    assert_eq!(ids, vec!["b2".repeat(32), root_id.clone()]);

    nmp_app_chirp_close_thread_feed(app, root_c.as_ptr());
    let gone = typed_projection_is_gone(app, &thread_feed_key(&root_id));
    assert!(gone, "thread feed projection must be gone after close");
    nmp_app_free(app);
}

#[test]
fn author_feed_open_emits_typed_op_feed_sidecar_and_close_removes_it() {
    let app = nmp_app_new();
    assert!(!app.is_null());
    let store = Arc::new(MemEventStore::new());
    let pubkey = "11".repeat(32);
    insert_raw(
        &store,
        RawEvent {
            id: "a1".repeat(32),
            pubkey: pubkey.clone(),
            created_at: 10,
            kind: 1,
            tags: vec![],
            content: "older".into(),
            sig: "a".repeat(128),
        },
    );
    insert_raw(
        &store,
        RawEvent {
            id: "a2".repeat(32),
            pubkey: pubkey.clone(),
            created_at: 20,
            kind: 1,
            tags: vec![],
            content: "newer".into(),
            sig: "a".repeat(128),
        },
    );
    install_store(app, store);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());

    let key = author_feed_key(&pubkey);
    let app_ref = unsafe { &*app };
    let typed = app_ref.run_typed_snapshot_projections();
    let entry = typed.iter().find(|p| p.key == key).expect("typed sidecar");

    assert_eq!(entry.schema_id, OP_FEED_SCHEMA_ID);
    assert_eq!(entry.schema_version, OP_FEED_SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NOFS");

    let snapshot = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload)
        .expect("typed payload decodes as a NOFS op-feed snapshot");
    let ids: Vec<String> = snapshot.cards.iter().map(|c| c.card.id.clone()).collect();
    assert_eq!(ids, vec!["a2".repeat(32), "a1".repeat(32)]);

    nmp_app_chirp_close_author_feed(app, pubkey_c.as_ptr());
    let typed_after = app_ref.run_typed_snapshot_projections();
    let clear = typed_after
        .iter()
        .find(|p| p.key == key)
        .expect("Cleared row");
    assert_eq!(clear.state, WireProjectionState::Cleared);
    assert!(clear.payload.is_empty());
    let typed_again = app_ref.run_typed_snapshot_projections();
    assert!(
        typed_again.iter().all(|p| p.key != key),
        "typed Cleared row must be one-shot"
    );
    nmp_app_free(app);
}

#[test]
fn author_feed_load_older_grows_visible_window_past_first_page() {
    // BLOCKING 2 (wiring): a `load_older` drain ingests older rows; the
    // `advance` closure grows the FlatFeed viewport so those rows become
    // user-visible in the EMITTED typed sidecar (which now reads
    // `snapshot_current_window`, not a fixed first page). Asserts the
    // emitted projection LENGTH grows after `load_older`, not merely that
    // rows were ingested.
    let app = nmp_app_new();
    assert!(!app.is_null());
    let store = Arc::new(MemEventStore::new());
    let pubkey = "11".repeat(32);
    let total = nmp_feed::DEFAULT_FEED_WINDOW_LIMIT + 20;
    for i in 0..total as u64 {
        insert_raw(
            &store,
            RawEvent {
                id: format!("{i:064x}"),
                pubkey: pubkey.clone(),
                created_at: 1_000 + i,
                kind: 1,
                tags: vec![],
                content: format!("n{i}"),
                sig: "a".repeat(128),
            },
        );
    }
    install_store(app, store);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);

    // First page only, despite all `total` rows being ingested (they sort
    // below the newest-first first page).
    let first = read_typed_card_ids(app, &key).expect("author feed projection");
    assert_eq!(
        first.len(),
        nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        "sidecar emits only the first page before load_older"
    );

    // load_older drains a page over the real pull substrate and the advance
    // closure grows the viewport → the older rows become visible.
    let app_ref = unsafe { &*app };
    assert!(
        app_ref.load_older_feed(&key),
        "load_older must drain + grow the viewport"
    );
    let grown = read_typed_card_ids(app, &key).expect("author feed projection after load_older");
    assert_eq!(
        grown.len(),
        total,
        "the previously-hidden older rows are now emitted"
    );
    assert!(grown.len() > first.len(), "the emitted projection grew");

    nmp_app_chirp_close_author_feed(app, pubkey_c.as_ptr());
    nmp_app_free(app);
}

fn install_store(app: *mut NmpApp, store: Arc<MemEventStore>) {
    let app_ref = unsafe { &*app };
    *app_ref.event_store_handle().lock().unwrap() = Some(store);
}

fn insert_raw(store: &MemEventStore, raw: RawEvent) {
    store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://seed.example/".to_string(),
            1_000,
        )
        .unwrap();
}

/// Return the decoded op-feed card IDs for `key` via the typed sidecar lane,
/// or `None` when the key is absent / cleared. Replaces the deleted generic
/// JSON lane (rule A6).
fn read_typed_card_ids(app: *mut NmpApp, key: &str) -> Option<Vec<String>> {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    let entry = projections.iter().find(|p| p.key == key && !p.payload.is_empty())?;
    let snapshot = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload).ok()?;
    Some(snapshot.cards.iter().map(|c| c.card.id.clone()).collect())
}

/// Return `true` when the typed sidecar for `key` is absent or cleared.
fn typed_projection_is_gone(app: *mut NmpApp, key: &str) -> bool {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    projections.iter().all(|p| p.key != key || p.payload.is_empty())
}
