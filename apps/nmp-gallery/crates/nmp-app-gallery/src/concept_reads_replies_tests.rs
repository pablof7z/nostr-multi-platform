//! DERISK PROOF (#2899): the checked-in "generated-shape"
//! `#[uniffi::export] impl GalleryApp` block in `concept_reads_replies.rs` —
//! a SEPARATE file/module from `facade.rs`'s `#[derive(uniffi::Object)]` and
//! its own `#[uniffi::export] impl GalleryApp` block — compiles and drives a
//! real end-to-end read: open → typed reply-summary frames decode → close →
//! output tombstoned. A UniFFI namespace/crate-resolution failure from the
//! split would be a hard compile error, so `cargo test -p nmp-app-gallery`
//! passing at all is already half the proof; this test exercises the runtime
//! behavior on top.
//!
//! Event injection uses `nmp_core::testing`'s `VerifiedEvent::
//! from_raw_unchecked` — the standard nmp-core test-support seam (see
//! `crates/nmp-native-runtime/src/op_feed_session/tests.rs`) — so this test
//! needs no `nostr` signing dependency; it drives the SAME
//! `TestSupportCommand::IngestPreVerifiedEvents` ingest path a signed event
//! would take, bypassing only cryptographic verification.

use std::time::Duration;

use nmp_core::actor::{ActorCommand, TestSupportCommand};
use nmp_core::testing::{RawEvent, VerifiedEvent};

use super::*;
use crate::facade::GalleryApp;

const TARGET_EVENT_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TARGET_AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REPLY_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REPLY_AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn target_json() -> String {
    format!(
        r#"{{"target_type":"event","event_id":"{TARGET_EVENT_ID}","kind":1,"author_pubkey":"{TARGET_AUTHOR}"}}"#
    )
}

fn reply_event() -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: REPLY_ID.to_string(),
        pubkey: REPLY_AUTHOR.to_string(),
        created_at: 1,
        kind: 1,
        tags: vec![vec![
            "e".to_string(),
            TARGET_EVENT_ID.to_string(),
            String::new(),
            "reply".to_string(),
        ]],
        content: "hi".to_string(),
        sig: "0".repeat(128),
    })
}

/// Run every registered typed-output encoder and return the ONE row for
/// `key`, if currently live. Mirrors the same accessor the read-lifecycle
/// engine's own frame-building tick uses
/// (`NmpApp::run_typed_snapshot_projections`), so this reads exactly what a
/// real update frame would carry for this projection — no separate test-only
/// decode path.
fn find_projection(app: &GalleryApp, key: &str) -> Option<nmp_core::TypedProjectionData> {
    app.runtime()
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key)
}

#[test]
fn open_replies_then_close_replies_round_trips_through_the_generated_shape_export() {
    let app = GalleryApp::new();
    app.start(64, 8);

    // OPEN — through the generated-shape `#[uniffi::export]` method, exactly
    // as a real UniFFI-generated Swift/Kotlin binding would call it.
    let opened = app
        .open_replies(target_json())
        .expect("a valid kind:1 event target opens a reply read");
    assert!(!opened.projection_key.is_empty());
    assert_ne!(opened.handle_id, 0);

    // DECODE (before any reply arrives) — the typed output is live from the
    // moment `open_replies` returns, with count 0.
    let before = find_projection(&app, &opened.projection_key)
        .expect("open_replies installs a live typed output immediately");
    let before_snapshot = nmp_replies::decode_reply_summary_snapshot(&before.payload)
        .expect("a valid REPLY_SUMMARY payload decodes");
    assert_eq!(before_snapshot.target_id, TARGET_EVENT_ID);
    assert_eq!(before_snapshot.count, 0);

    // A real reply event flows through the SAME ingest path a relay-sourced
    // event would (`IngestPreVerifiedEvents`), then the actor barrier proves
    // it has been folded into the reducer before we read again.
    app.runtime().send_cmd(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEvents(vec![reply_event()]),
    ));
    assert!(
        app.runtime()
            .wait_barrier_for_test(Duration::from_secs(5)),
        "the actor must drain the injected reply before we re-read the projection"
    );

    // DECODE (after the reply) — the reducer's admission folds it in and the
    // typed output reflects it on the next synchronous read.
    let after = find_projection(&app, &opened.projection_key)
        .expect("the read is still live after ingest");
    let after_snapshot = nmp_replies::decode_reply_summary_snapshot(&after.payload)
        .expect("a valid REPLY_SUMMARY payload decodes");
    assert_eq!(after_snapshot.count, 1, "the injected reply is admitted");
    assert_eq!(after_snapshot.reply_event_ids, vec![REPLY_ID.to_string()]);

    // CLOSE — through the generated-shape export, reconstructing the typed
    // handle from the SAME scalar parts `open_replies` returned (no
    // facade-owned handle map anywhere in `concept_reads_replies.rs`).
    assert!(app.close_replies(opened.clone()));

    // TOMBSTONED — `SnapshotRegistry::remove` (nmp-core) emits ONE pending
    // `Cleared` row for the removed key, then omits it entirely from every
    // subsequent tick. So the row returned by the FIRST post-close read (if
    // any) must be the `Cleared` tombstone, never a live `Changed` payload,
    // and the row must be gone outright on the NEXT read.
    match find_projection(&app, &opened.projection_key) {
        None => {}
        Some(row) => assert_eq!(
            row.state,
            nmp_core::WireProjectionState::Cleared,
            "a row surviving one tick past close must be the one-shot tombstone, not a live payload"
        ),
    }
    assert!(
        find_projection(&app, &opened.projection_key).is_none(),
        "the one-shot Cleared tombstone must be fully drained by the second read"
    );

    // D6 idempotency: closing again (e.g. a retried facade call) is a safe
    // no-op, never a panic.
    assert!(!app.close_replies(opened));

    app.stop();
}

#[test]
fn open_replies_rejects_a_malformed_target_json() {
    let app = GalleryApp::new();
    let err = app
        .open_replies("{not json}".to_string())
        .expect_err("malformed target_json must be rejected");
    assert_eq!(err, GalleryReadError::InvalidTarget);
}

#[test]
fn open_replies_rejects_a_kind_1111_event_shape_target() {
    // Mirrors the concept-crate-level marshal proof (#2899 DERISK refocus):
    // the facade surface must not silently accept a bare `event` shape for a
    // kind:1111 target either — it propagates the SAME rejection
    // `nmp_replies::decode_and_validate_reply_target` returns.
    let app = GalleryApp::new();
    let json = format!(
        r#"{{"target_type":"event","event_id":"{TARGET_EVENT_ID}","kind":1111,"author_pubkey":"{TARGET_AUTHOR}"}}"#
    );
    let err = app
        .open_replies(json)
        .expect_err("a kind:1111 target must use the comment shape");
    assert_eq!(err, GalleryReadError::InvalidTarget);
}

#[test]
fn close_replies_on_an_unknown_handle_is_a_safe_noop() {
    let app = GalleryApp::new();
    app.start(64, 8);
    let unknown = GalleryOpenedReplies {
        projection_key: "nmp.replies.summary.unknown.0".to_string(),
        handle_id: 999_999,
    };
    assert!(!app.close_replies(unknown));
    app.stop();
}
