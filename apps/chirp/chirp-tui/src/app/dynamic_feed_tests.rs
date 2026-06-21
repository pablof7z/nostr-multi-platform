use super::*;
use crate::bridge::{NmpEvent, UpdatePayload};
use crate::snapshot::FeedProjection;
use crate::timeline::TimelineRow;
use std::cell::RefCell;
use std::collections::HashMap;

const HOME_AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROFILE_AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const THREAD_ROOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn feed_value(id: &str, author: &str) -> serde_json::Value {
    serde_json::json!({
        "cards": [{
            "card": {
                "id": id,
                "author_pubkey": author,
                "kind": 1,
                "created_at": 1_700_000_000_u64,
                "content": format!("feed row {id}"),
                "content_tree": { "nodes": [], "roots": [], "mode": "Plain" },
                "relation_counts": {
                    "replies": { "state": "known", "count": 0 },
                    "reactions": { "state": "known", "count": 0 },
                    "reposts": { "state": "known", "count": 0 },
                    "comments": { "state": "known", "count": 0 },
                    "zaps": { "state": "known", "count": 0 }
                }
            },
            "attribution": []
        }],
        "page": { "limit": 20, "has_more": false, "total_blocks": 1 },
        "metrics": null
    })
}

fn nofs_projection(key: &str, id: &str, author: &str) -> nmp_core::TypedProjectionData {
    let source = feed_value(id, author);
    let typed: nmp_nip01::OpFeedSnapshot =
        serde_json::from_value(source).expect("test value is an OP feed snapshot");
    nmp_core::TypedProjectionData {
        key: key.to_string(),
        schema_id: nmp_nip01::OP_FEED_SCHEMA_ID.to_string(),
        schema_version: nmp_nip01::OP_FEED_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(nmp_nip01::OP_FEED_FILE_IDENTIFIER).into_owned(),
        payload: nmp_nip01::encode_op_feed_snapshot(&typed),
        ..Default::default()
    }
}

#[derive(Default)]
struct RecordingRuntime {
    calls: RefCell<Vec<String>>,
}

impl dynamic_feeds::DynamicFeedRuntime for RecordingRuntime {
    fn open_author(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("open_author:{pubkey}"));
        Ok(())
    }

    fn close_author(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("close_author:{pubkey}"));
        Ok(())
    }

    fn open_thread(&self, event_id: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("open_thread:{event_id}"));
        Ok(())
    }

    fn close_thread(&self, event_id: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("close_thread:{event_id}"));
        Ok(())
    }

    fn resolve_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("resolve_open_profile:{pubkey}"));
        Ok(())
    }

    fn release_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("release_open_profile:{pubkey}"));
        Ok(())
    }
}

fn feed_map(key: &str, projection: FeedProjection) -> HashMap<String, FeedProjection> {
    HashMap::from([(key.to_string(), projection)])
}

fn event_with_feeds(feeds: &[nmp_core::TypedProjectionData]) -> NmpEvent {
    NmpEvent {
        payload: UpdatePayload::FlatBuffers(nmp_core::encode_snapshot_frame(
            &nmp_core::SnapshotEnvelope::default(),
            feeds,
        )),
    }
}

#[test]
fn profile_rows_come_from_author_feed_not_home_feed() {
    let (runtime, _rx) = AppRuntime::new().expect("runtime starts");
    let mut state = AppState {
        profile_pubkey: PROFILE_AUTHOR.to_string(),
        ..Default::default()
    };
    let author_key = format!("nmp.feed.author.{PROFILE_AUTHOR}");
    let event = event_with_feeds(&[
        nofs_projection("nmp.feed.home", "home-row", HOME_AUTHOR),
        nofs_projection(&author_key, "author-row", PROFILE_AUTHOR),
    ]);

    state.apply_nmp_event(&runtime, event);

    assert_eq!(state.rows[0].id, "home-row");
    assert_eq!(state.rows[0].author_pubkey, HOME_AUTHOR);
    assert_eq!(state.profile_rows[0].id, "author-row");
    assert_eq!(state.profile_rows[0].author_pubkey, PROFILE_AUTHOR);
}

#[test]
fn thread_rows_come_from_thread_feed_not_home_feed() {
    let (runtime, _rx) = AppRuntime::new().expect("runtime starts");
    let mut state = AppState {
        thread_event_id: THREAD_ROOT.to_string(),
        detail_cursor: 99,
        ..Default::default()
    };
    let thread_key = format!("nmp.feed.thread.{THREAD_ROOT}");
    let event = event_with_feeds(&[
        nofs_projection("nmp.feed.home", "home-row", HOME_AUTHOR),
        nofs_projection(&thread_key, "thread-row", PROFILE_AUTHOR),
    ]);

    state.apply_nmp_event(&runtime, event);

    assert_eq!(state.rows[0].id, "home-row");
    assert_eq!(state.thread_rows[0].id, "thread-row");
    assert_eq!(state.detail_cursor, 0);
}

#[test]
fn opening_author_b_closes_a_and_drops_a_rows() {
    let runtime = RecordingRuntime::default();
    let author_a = "11".repeat(32);
    let author_b = "22".repeat(32);
    let author_a_key = format!("nmp.feed.author.{author_a}");
    let author_b_key = format!("nmp.feed.author.{author_b}");
    let mut state = AppState::default();

    state.open_author_feed(&runtime, &author_a).expect("open A");
    state.apply_dynamic_feeds(&feed_map(
        &author_a_key,
        FeedProjection::Changed(feed_value("author-a-row", &author_a)),
    ));
    assert_eq!(state.profile_rows[0].id, "author-a-row");
    assert_eq!(state.profile_rows[0].author_pubkey, author_a);

    state.open_author_feed(&runtime, &author_b).expect("open B");
    assert!(
        state.profile_rows.is_empty(),
        "opening B must tear down A rows before B renders"
    );

    state.apply_dynamic_feeds(&HashMap::from([
        (author_a_key, FeedProjection::Cleared),
        (
            author_b_key,
            FeedProjection::Changed(feed_value("author-b-row", &author_b)),
        ),
    ]));

    assert_eq!(
        runtime.calls.borrow().as_slice(),
        &[
            // ADR-0063 (#1671 Lane F): each open resolves the open-pane profile at
            // profile.card/Live; each close releases it (before opening the next).
            format!("open_author:{author_a}"),
            format!("resolve_open_profile:{author_a}"),
            format!("close_author:{author_a}"),
            format!("release_open_profile:{author_a}"),
            format!("open_author:{author_b}"),
            format!("resolve_open_profile:{author_b}"),
        ],
        "opening a new author feed must close the prior kernel sidecar before opening the next"
    );
    assert_eq!(state.profile_pubkey, author_b);
    assert_eq!(state.profile_rows.len(), 1);
    assert_eq!(state.profile_rows[0].id, "author-b-row");
    assert_eq!(state.profile_rows[0].author_pubkey, author_b);
    assert!(
        state
            .profile_rows
            .iter()
            .all(|row| row.id != "author-a-row"),
        "A's row must not survive after B opens"
    );
}

#[test]
fn cleared_author_feed_removes_active_profile_rows() {
    let author = "33".repeat(32);
    let author_key = format!("nmp.feed.author.{author}");
    let mut state = AppState {
        focused: Pane::Profile,
        profile_pubkey: author.clone(),
        ..Default::default()
    };
    state.apply_dynamic_feeds(&feed_map(
        &author_key,
        FeedProjection::Changed(feed_value("open-row", &author)),
    ));
    assert_eq!(state.profile_rows[0].id, "open-row");

    state.apply_dynamic_feeds(&feed_map(&author_key, FeedProjection::Cleared));

    assert!(state.profile_pubkey.is_empty());
    assert!(state.profile_rows.is_empty());
    assert_eq!(state.focused, Pane::Feed);
}

#[test]
fn close_current_thread_view_clears_rows_and_dispatches_kernel_close() {
    let runtime = RecordingRuntime::default();
    let thread = "44".repeat(32);
    let mut state = AppState {
        focused: Pane::Detail,
        thread_event_id: thread.clone(),
        thread_rows: TimelineRow::from_snapshot(&feed_value("thread-row", PROFILE_AUTHOR)),
        detail_cursor: 1,
        detail_scroll: 3,
        ..Default::default()
    };

    let closed = state
        .close_current_dynamic_view(&runtime)
        .expect("close succeeds");

    assert_eq!(closed.thread.as_deref(), Some(thread.as_str()));
    assert_eq!(
        runtime.calls.borrow().as_slice(),
        &[format!("close_thread:{thread}")],
        "closing the visible thread must call the kernel close API with that event id"
    );
    assert!(state.thread_event_id.is_empty());
    assert!(state.thread_rows.is_empty());
    assert_eq!(state.detail_cursor, 0);
    assert_eq!(state.detail_scroll, 0);
    assert_eq!(state.focused, Pane::Feed);
}
