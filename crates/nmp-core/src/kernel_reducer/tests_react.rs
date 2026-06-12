//! Tests for [`KernelReducer::build_reaction_draft`].
//!
//! `build_reaction_draft` is the PR-6a wasm write-path seam for NIP-25
//! kind:7 reactions. These tests verify the hex-validation contract, the
//! D6 graceful degradation when the target event's author is not in the
//! read-cache, and the `p`-tag inclusion when the author IS cached.

use super::*;
use crate::store::{RawEvent, VerifiedEvent};

// ─── Synthetic event IDs (valid 64-char hex) ────────────────────────────────

const TARGET_ID: &str =
    "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const TARGET_AUTHOR: &str =
    "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn build_reaction_draft_returns_none_for_invalid_hex() {
    let r = KernelReducer::new();
    assert!(r.build_reaction_draft("not-hex", "+").is_none());
    assert!(r.build_reaction_draft("deadbeef", "+").is_none()); // too short
}

#[test]
fn build_reaction_draft_degrades_gracefully_when_author_not_cached() {
    // Valid 64-char hex id but target event is absent from read-cache —
    // build_reaction_draft returns e-tag only (valid NIP-25, D6).
    let r = KernelReducer::new();
    let (tags, content) = r
        .build_reaction_draft(TARGET_ID, "+")
        .expect("valid hex id should return Some");
    assert_eq!(tags.len(), 1, "only e-tag expected when author not cached");
    assert_eq!(tags[0][0], "e");
    assert_eq!(tags[0][1], TARGET_ID);
    assert_eq!(content, "+");
}

#[test]
fn build_reaction_draft_includes_p_tag_when_author_cached() {
    // ingest_pre_verified_event populates BOTH the store AND self.events (the
    // HashMap read-cache that event_author reads). Store-only seeding (as used
    // by seed_event_with_kind in tests_reply_tags) is insufficient here
    // because event_author reads self.events, not the store.
    let mut r = KernelReducer::new();
    let raw = RawEvent {
        id: TARGET_ID.to_string(),
        pubkey: TARGET_AUTHOR.to_string(),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: "hello".into(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    r.kernel
        .ingest_pre_verified_event(crate::relay::RelayRole::Content, "sub-test", verified);

    let (tags, content) = r
        .build_reaction_draft(TARGET_ID, "+")
        .expect("valid hex id should return Some after ingest");
    assert_eq!(tags.len(), 2, "e-tag + p-tag expected when author is cached");
    assert_eq!(tags[0][0], "e");
    assert_eq!(tags[0][1], TARGET_ID);
    assert_eq!(tags[1][0], "p");
    assert_eq!(tags[1][1], TARGET_AUTHOR);
    assert_eq!(content, "+");
}

#[test]
fn build_reaction_draft_normalises_blank_reaction_to_plus() {
    let r = KernelReducer::new();
    let (_, content) = r
        .build_reaction_draft(TARGET_ID, "")
        .expect("valid hex id");
    assert_eq!(content, "+", "empty string must normalise to '+'");
    let (_, content2) = r
        .build_reaction_draft(TARGET_ID, "   ")
        .expect("valid hex id");
    assert_eq!(content2, "+", "whitespace-only must normalise to '+'");
}

#[test]
fn build_reaction_draft_passes_through_emoji_reaction() {
    let r = KernelReducer::new();
    let (_, content) = r
        .build_reaction_draft(TARGET_ID, "🤙")
        .expect("valid hex id");
    assert_eq!(content, "🤙");
}
