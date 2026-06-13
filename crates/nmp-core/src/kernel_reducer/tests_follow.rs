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
