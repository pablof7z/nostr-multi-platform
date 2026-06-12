//! Tests for [`KernelReducer::build_reply_tags`].
//!
//! `build_reply_tags` is the wasm write-path seam: it looks up an event from
//! the kernel's store, parses its NIP-10 refs, and delegates tag assembly to
//! `crate::tags::reply_tags`. These tests verify the store-lookup contract
//! and the NIP-10 correctness of the returned tags.

use super::*;
use crate::store::{RawEvent, VerifiedEvent};

// ─── Synthetic event IDs (valid 64-char hex) ────────────────────────────────

/// A parent event that IS the thread root (no root ref in its tags).
const PARENT_ROOT_ID: &str =
    "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const PARENT_ROOT_AUTHOR: &str =
    "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

/// A mid-thread event that carries a root ref.
const PARENT_MID_ID: &str =
    "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
const PARENT_MID_AUTHOR: &str =
    "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";
const ROOT_ID: &str =
    "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5";
const ALICE_PK: &str =
    "f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6";

// ─── Helpers ────────────────────────────────────────────────────────────────

fn seed_event_with_kind(
    r: &KernelReducer,
    id: &str,
    pubkey: &str,
    kind: u32,
    tags: Vec<Vec<String>>,
) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind,
        tags,
        content: "test".into(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    r.kernel
        .event_store_handle()
        .insert(verified, &"wss://seed".to_string(), 0)
        .expect("seed insert");
}

fn seed_event(r: &KernelReducer, id: &str, pubkey: &str, tags: Vec<Vec<String>>) {
    seed_event_with_kind(r, id, pubkey, 1, tags);
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn build_reply_tags_returns_none_for_missing_event() {
    let r = KernelReducer::new();
    // No events seeded — any valid hex id returns None.
    assert!(r.build_reply_tags(PARENT_ROOT_ID).is_none());
}

#[test]
fn build_reply_tags_returns_none_for_invalid_hex() {
    let r = KernelReducer::new();
    assert!(r.build_reply_tags("not-hex").is_none());
    assert!(r.build_reply_tags("deadbeef").is_none()); // too short
}

#[test]
fn build_reply_tags_for_root_event_uses_parent_as_both_root_and_reply() {
    let r = KernelReducer::new();
    seed_event(&r, PARENT_ROOT_ID, PARENT_ROOT_AUTHOR, vec![]);

    let tags = r.build_reply_tags(PARENT_ROOT_ID).expect("event seeded");
    // 2 e-tags + 1 p-tag
    assert_eq!(tags.len(), 3);
    // root e-tag points at parent
    assert_eq!(tags[0][0], "e");
    assert_eq!(tags[0][1], PARENT_ROOT_ID);
    assert_eq!(tags[0][3], "root");
    // reply e-tag also points at parent (it IS the root)
    assert_eq!(tags[1][0], "e");
    assert_eq!(tags[1][1], PARENT_ROOT_ID);
    assert_eq!(tags[1][3], "reply");
    // p-tag → parent author
    assert_eq!(tags[2][0], "p");
    assert_eq!(tags[2][1], PARENT_ROOT_AUTHOR);
}

#[test]
fn build_reply_tags_for_mid_thread_event_inherits_root_ref() {
    let r = KernelReducer::new();
    // Mid-thread event: carries a root e-tag and a p-tag for alice.
    let event_tags = vec![
        vec!["e".into(), ROOT_ID.into(), "".into(), "root".into()],
        vec!["p".into(), ALICE_PK.into()],
    ];
    seed_event(&r, PARENT_MID_ID, PARENT_MID_AUTHOR, event_tags);

    let tags = r.build_reply_tags(PARENT_MID_ID).expect("event seeded");
    // root e-tag → ROOT_ID
    assert_eq!(tags[0][1], ROOT_ID, "root e-tag must carry thread root id");
    assert_eq!(tags[0][3], "root");
    // reply e-tag → PARENT_MID_ID
    assert_eq!(tags[1][1], PARENT_MID_ID, "reply e-tag must carry direct parent");
    assert_eq!(tags[1][3], "reply");
    // p-tags: parent author first, then alice
    assert_eq!(tags[2][1], PARENT_MID_AUTHOR);
    assert_eq!(tags[3][1], ALICE_PK);
}

#[test]
fn build_reply_tags_returns_none_for_non_note_parent() {
    // kind:0 (profile metadata) is in the store but is not a valid reply
    // target. build_reply_tags must fail closed, matching native NoteRecord
    // domain which is kind:1-only.
    let r = KernelReducer::new();
    seed_event_with_kind(&r, PARENT_ROOT_ID, PARENT_ROOT_AUTHOR, 0, vec![]);
    assert!(
        r.build_reply_tags(PARENT_ROOT_ID).is_none(),
        "replying to a kind:0 event must fail closed"
    );
}
