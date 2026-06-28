//! #1740 read lane — the `FeedScope::Authors` static-author-set scope over the
//! REAL op-feed composition (`compile_feed_params` via `NmpApp::open_feed`).
//!
//! Proves the new closed-algebra variant opens over the session engine (an
//! `AdmitExpr::Authors` admission gate + a fixed author+kind acquisition) and
//! tears down idempotently by HANDLE, and that an EMPTY author set fails CLOSED
//! (never silently opens a feed that admits everyone) registering nothing. The
//! per-author ADMISSION proof (only the target author's events are admitted, a
//! non-author's are excluded) lives as a pure unit test in
//! `session_compile::resolve_tests`.

use std::sync::{Arc, Mutex};

use nmp_core::WireProjectionState;
mod common;
use common::*;

use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ProjectionKey,
};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

// One live `NmpApp` at a time — the harness-contention guard the sibling
// op-feed integration tests use.
static SERIAL: Mutex<()> = Mutex::new(());

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const ROOT_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RELAY: &str = "wss://test.relay/";

fn set_app_active(app: *mut NmpApp, active: Option<&str>) {
    // SAFETY: tests pass a live pointer returned by `nmp_app_new`.
    let handle = unsafe { &*app }.active_account_handle();
    *handle.lock().expect("active-account slot") = active.map(str::to_string);
}

fn author_params(authors: &[&str], projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Authors {
            authors: authors.iter().map(|s| (*s).to_string()).collect(),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
    }
}

fn referrer_params(event_id: &str, projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Referrer {
            event_id: event_id.to_string(),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
    }
}

fn raw_event(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind,
        tags,
        content: format!("event {id}"),
        sig: "00".repeat(64),
    }
}

fn publish_store(app: &NmpApp, events: Vec<RawEvent>) {
    let store = Arc::new(MemEventStore::new());
    for raw in events {
        store
            .insert(
                VerifiedEvent::from_raw_unchecked(raw),
                &RELAY.to_string(),
                1_000,
            )
            .expect("seed store insert");
    }
    let store: Arc<dyn EventStore> = store;
    *app.event_store_handle().lock().expect("event-store slot") = Some(store);
}

fn flat_feed_ids(app: &NmpApp, key: &str) -> Vec<String> {
    let row = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key && row.state != WireProjectionState::Cleared)
        .expect("flat feed typed projection");
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NOFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}

fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_native_runtime::compile_feed_params(app, params, kinds)
}

#[test]
fn authors_flat_open_replays_cached_primary_kind_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = new_app_ptr();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let note_id = "2222222222222222222222222222222222222222222222222222222222222222";
    let longform_id = "3333333333333333333333333333333333333333333333333333333333333333";
    publish_store(
        app_ref,
        vec![
            raw_event(note_id, BOB, 1, 100, Vec::new()),
            raw_event(longform_id, BOB, 30_023, 101, Vec::new()),
        ],
    );

    let params = author_params(&[BOB], "test.feed.author.cached");
    let _handle = app_ref
        .open_feed(&params, &compiler)
        .expect("author flat feed opens");

    assert_eq!(
        flat_feed_ids(app_ref, "test.feed.author.cached"),
        vec![note_id.to_string()],
        "open_feed must replay cached author rows immediately and reject non-primary kinds"
    );

    free_app_ptr(app);
}

#[test]
fn referrer_flat_open_replays_cached_root_and_replies() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = new_app_ptr();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let reply_id = "4444444444444444444444444444444444444444444444444444444444444444";
    let wrong_kind_id = "5555555555555555555555555555555555555555555555555555555555555555";
    let root_tag = vec![vec!["e".to_string(), ROOT_ID.to_string()]];
    publish_store(
        app_ref,
        vec![
            raw_event(ROOT_ID, ALICE, 1, 100, Vec::new()),
            raw_event(reply_id, BOB, 1, 101, root_tag.clone()),
            raw_event(wrong_kind_id, BOB, 30_023, 102, root_tag),
        ],
    );

    let params = referrer_params(ROOT_ID, "test.feed.thread.cached");
    let _handle = app_ref
        .open_feed(&params, &compiler)
        .expect("thread flat feed opens");

    assert_eq!(
        flat_feed_ids(app_ref, "test.feed.thread.cached"),
        vec![reply_id.to_string(), ROOT_ID.to_string()],
        "open_feed must replay cached thread root/replies immediately and reject non-primary kinds"
    );

    free_app_ptr(app);
}

#[test]
fn authors_scope_opens_and_closes_over_session_engine() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = new_app_ptr();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    let params = author_params(&[BOB], "test.feed.author");
    let handle = app_ref
        .open_feed(&params, &compiler)
        .expect("Authors scope opens over the session engine");
    assert_eq!(
        handle.projection_key,
        ProjectionKey("test.feed.author".into())
    );
    assert!(app_ref.feed_session_is_open(&handle), "session live");
    assert_eq!(app_ref.live_feed_session_count(), 1);
    assert!(
        app_ref
            .run_typed_snapshot_projections()
            .iter()
            .any(|p| p.key == "test.feed.author"),
        "session sidecar emitted under the unique key"
    );

    // Handle-close tears it down (withdraws the author interest, removes the
    // controller + projection, revokes the engine observer) — idempotently.
    assert!(app_ref.close_feed(&handle), "close tears the session down");
    assert!(!app_ref.feed_session_is_open(&handle));
    assert_eq!(app_ref.live_feed_session_count(), 0, "no live sessions");
    assert!(!app_ref.close_feed(&handle), "second close is a no-op");

    free_app_ptr(app);
}

#[test]
fn empty_authors_scope_fails_closed_and_registers_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = new_app_ptr();
    set_app_active(app, Some(ALICE));
    let app_ref: &NmpApp = unsafe { &*app };

    // An EMPTY author set must fail closed — never silently open a feed that
    // admits everyone — and register nothing (no leak).
    let params = author_params(&[], "test.feed.author-empty");
    let err = app_ref
        .open_feed(&params, &compiler)
        .expect_err("an empty author set must fail closed");
    assert!(
        matches!(err, FeedOpenError::ScopeNotSupportedYet { scope } if scope == "Authors-empty-set"),
        "typed fail-closed error naming the empty author set, got {err:?}"
    );
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "no session leaked for a fail-closed empty-author open"
    );
    assert!(
        !app_ref
            .run_typed_snapshot_projections()
            .iter()
            .any(|p| p.key == "test.feed.author-empty"),
        "no sidecar registered for a fail-closed open"
    );

    free_app_ptr(app);
}
