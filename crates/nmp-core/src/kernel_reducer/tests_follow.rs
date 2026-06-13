//! Tests for [`KernelReducer::try_current_follows`].
//!
//! `try_current_follows` is the PR-6b wasm write-path seam: it reads the
//! active account's kind:3 contact list from the store, distinguishing
//! "not loaded" (`None`) from "loaded but empty" (`Some([])`). These tests
//! verify all three states: not-loaded, loaded-empty, and loaded-non-empty.

use super::*;
use crate::store::{RawEvent, VerifiedEvent};

// ─── Synthetic pubkeys (valid 64-char hex) ───────────────────────────────────

const ACCOUNT_PK: &str =
    "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const FOLLOW_A: &str =
    "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const FOLLOW_B: &str =
    "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";
// A kind:3 event id (arbitrary valid 64-char hex)
const KIND3_ID: &str =
    "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Set the active account on the reducer (mirrors how wasm runtime does it).
fn set_active_account(r: &mut KernelReducer, pubkey_hex: &str) {
    r.set_active_account(pubkey_hex.to_string());
}

/// Seed a kind:3 event for `pubkey` with a given set of followed pubkeys
/// into the kernel store.
fn seed_kind3(r: &KernelReducer, author: &str, follows: &[&str]) {
    let tags: Vec<Vec<String>> = follows
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    let raw = RawEvent {
        id: KIND3_ID.to_string(),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 3,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    r.kernel
        .event_store_handle()
        .insert(verified, &"wss://seed".to_string(), 0)
        .expect("kind:3 seed insert");
}

/// Seed a kind:3 event for `author` with a verbatim tag set and content into
/// the kernel store. Unlike [`seed_kind3`] this carries the FULL tag shape
/// (relay hints, petnames, non-`p` tags) and a non-empty `content`, so the
/// issue-#1246 preservation contract can be exercised end-to-end.
fn seed_kind3_raw(r: &KernelReducer, author: &str, tags: Vec<Vec<String>>, content: &str) {
    let raw = RawEvent {
        id: KIND3_ID.to_string(),
        pubkey: author.to_string(),
        created_at: 1_700_000_000,
        kind: 3,
        tags,
        content: content.to_string(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    r.kernel
        .event_store_handle()
        .insert(verified, &"wss://seed".to_string(), 0)
        .expect("kind:3 seed insert");
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn try_current_follows_returns_none_when_no_active_account() {
    // No active account set → None, not empty.
    let r = KernelReducer::new();
    assert!(
        r.try_current_follows().is_none(),
        "no active account must return None"
    );
}

#[test]
fn try_current_follows_returns_none_when_kind3_not_loaded() {
    // Active account is set but no kind:3 has been ingested yet → None.
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    assert!(
        r.try_current_follows().is_none(),
        "kind:3 not yet loaded must return None (not empty)"
    );
}

#[test]
fn try_current_follows_returns_some_empty_when_kind3_loaded_with_no_follows() {
    // Active account + kind:3 ingested but zero p-tags → Some([]).
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    seed_kind3(&r, ACCOUNT_PK, &[]);
    let follows = r
        .try_current_follows()
        .expect("kind:3 loaded → must return Some, not None");
    assert!(
        follows.is_empty(),
        "kind:3 with no p-tags must return Some([]), not None"
    );
}

#[test]
fn try_current_follows_returns_some_with_follows_when_kind3_loaded() {
    // Active account + kind:3 with [A, B] → Some([A, B]).
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    seed_kind3(&r, ACCOUNT_PK, &[FOLLOW_A, FOLLOW_B]);
    let follows = r
        .try_current_follows()
        .expect("kind:3 loaded with follows → must return Some");
    assert_eq!(follows.len(), 2);
    assert!(follows.contains(&FOLLOW_A.to_string()));
    assert!(follows.contains(&FOLLOW_B.to_string()));
}

// ─── issue #1246: full-event seam + preservation contract ────────────────────

const RELAY_A: &str = "wss://relay.a.example";
const PETNAME_A: &str = "alice";

/// Build a richly-shaped kind:3 tag set: a non-`p` legacy tag, a `p` entry
/// with both a relay hint AND a petname, and a bare `p` entry — exactly the
/// shape the old pubkey-only rebuild silently flattened (issue #1246).
fn rich_kind3_tags() -> Vec<Vec<String>> {
    vec![
        // Non-`p` tag a richer client may carry (NIP-65-style relay list).
        vec!["r".to_string(), RELAY_A.to_string(), "read".to_string()],
        // `p` with relay hint (col 2) + petname (col 3).
        vec![
            "p".to_string(),
            FOLLOW_A.to_string(),
            RELAY_A.to_string(),
            PETNAME_A.to_string(),
        ],
        // Bare `p`.
        vec!["p".to_string(), FOLLOW_B.to_string()],
    ]
}

#[test]
fn try_current_kind3_event_returns_full_tags_and_content() {
    // The seam must hand back EVERY tag verbatim (non-`p`, relay hints,
    // petnames) plus the original content — not the pubkey-only projection.
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    seed_kind3_raw(&r, ACCOUNT_PK, rich_kind3_tags(), "legacy relay json");

    let (tags, content) = r
        .try_current_kind3_event()
        .expect("kind:3 loaded → must return Some");
    assert_eq!(tags, rich_kind3_tags(), "every tag must survive verbatim");
    assert_eq!(content, "legacy relay json", "content must survive verbatim");
}

#[test]
fn try_current_kind3_event_fails_closed_when_not_loaded() {
    // No kind:3 ingested → None (fail closed), never an empty event.
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    assert!(
        r.try_current_kind3_event().is_none(),
        "kind:3 not loaded must return None, not an empty event"
    );
}

#[test]
fn kind3_edit_add_preserves_relay_hints_petnames_non_p_and_content() {
    // The full edit pipeline (seam → canonical add editor) must append the new
    // follow WITHOUT disturbing the non-`p` tag, the relay-hinted+petnamed `p`,
    // the bare `p`, or the content (issue #1246a).
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    seed_kind3_raw(&r, ACCOUNT_PK, rich_kind3_tags(), "legacy relay json");

    let (tags, content) = r.try_current_kind3_event().expect("loaded");
    let new_target = "f".repeat(64);
    let edited = crate::tags::kind3_tags_after_add(&tags, &new_target);

    // Non-`p` tag survives verbatim, in place.
    assert!(
        edited.contains(&vec!["r".to_string(), RELAY_A.to_string(), "read".to_string()]),
        "non-`p` tag must survive an add edit"
    );
    // Relay hint + petname survive on the existing follow.
    assert!(
        edited.contains(&vec![
            "p".to_string(),
            FOLLOW_A.to_string(),
            RELAY_A.to_string(),
            PETNAME_A.to_string(),
        ]),
        "relay hint + petname must survive an add edit"
    );
    // The bare existing follow survives.
    assert!(edited.contains(&vec!["p".to_string(), FOLLOW_B.to_string()]));
    // The new follow is appended.
    assert!(edited.contains(&vec!["p".to_string(), new_target.clone()]));
    // Content is carried unchanged into the re-published event.
    assert_eq!(content, "legacy relay json");
}

#[test]
fn kind3_edit_remove_preserves_relay_hints_petnames_non_p_and_content() {
    // Removing a follow must drop ONLY its `p` entry, leaving the non-`p` tag,
    // the OTHER follow's relay hint + petname, and the content intact
    // (issue #1246a).
    let mut r = KernelReducer::new();
    set_active_account(&mut r, ACCOUNT_PK);
    seed_kind3_raw(&r, ACCOUNT_PK, rich_kind3_tags(), "legacy relay json");

    let (tags, content) = r.try_current_kind3_event().expect("loaded");
    // Remove the bare follow B; A (relay-hinted + petnamed) must remain.
    let edited = crate::tags::kind3_tags_after_remove(&tags, FOLLOW_B);

    assert!(
        !edited
            .iter()
            .any(|t| t.first().map(String::as_str) == Some("p")
                && t.get(1).map(String::as_str) == Some(FOLLOW_B)),
        "removed follow must be gone"
    );
    // Non-`p` tag survives.
    assert!(
        edited.contains(&vec!["r".to_string(), RELAY_A.to_string(), "read".to_string()]),
        "non-`p` tag must survive a remove edit"
    );
    // The kept follow retains its relay hint + petname.
    assert!(
        edited.contains(&vec![
            "p".to_string(),
            FOLLOW_A.to_string(),
            RELAY_A.to_string(),
            PETNAME_A.to_string(),
        ]),
        "kept follow's relay hint + petname must survive a remove edit"
    );
    assert_eq!(content, "legacy relay json");
}

#[test]
fn kind3_edit_remove_drops_relay_hinted_and_petnamed_p_of_any_arity() {
    // The remove editor must drop a `p` matched on its pubkey regardless of
    // arity — a relay-hinted, petnamed `p` for the target must disappear.
    let tags = rich_kind3_tags();
    let edited = crate::tags::kind3_tags_after_remove(&tags, FOLLOW_A);
    assert!(
        !edited
            .iter()
            .any(|t| t.first().map(String::as_str) == Some("p")
                && t.get(1).map(String::as_str) == Some(FOLLOW_A)),
        "a relay-hinted + petnamed `p` must be removable by pubkey"
    );
    // FOLLOW_B and the non-`p` tag stay.
    assert!(edited.contains(&vec!["p".to_string(), FOLLOW_B.to_string()]));
    assert!(edited.contains(&vec!["r".to_string(), RELAY_A.to_string(), "read".to_string()]));
}

#[test]
fn kind3_edit_add_is_idempotent_preserving_existing_columns() {
    // Re-adding an already-followed pubkey must NOT append a duplicate and must
    // leave the existing entry's relay hint + petname untouched.
    let tags = rich_kind3_tags();
    let edited = crate::tags::kind3_tags_after_add(&tags, FOLLOW_A);
    assert_eq!(edited, tags, "idempotent add must return the set unchanged");
}
