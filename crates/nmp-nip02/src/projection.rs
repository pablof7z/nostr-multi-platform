//! `FollowListProjection` — the active account's NIP-02 follow list.
//!
//! # Overview
//!
//! Exposes the active account's follows through
//! [`FollowListProjection::snapshot_json`] and [`FollowListProjection::snapshot`]
//! — the shapes a host `register_snapshot_projection` closure returns.
//!
//! # Root of truth
//!
//! The canonical kind:3 follow state is derived from the latest kind:3 event in
//! the kernel event store. This projection is a **thin read-model** over that
//! single source of truth: `snapshot()` reads the active account's latest kind:3
//! and maps the raw hex pubkeys to [`FollowEntry`] values. No secondary
//! `HashMap` is maintained.
//!
//! # Why the old `ObservedProjectionSink` approach was broken
//!
//! The prior design kept an observer-local `HashMap` populated only by
//! `ObservedProjectionSink::on_kernel_event`. This missed the startup cache-serve
//! that runs before the lazily-registered observer exists (continuation.rs:95-100
//! — no-double-dispatch rule is locked by test), and also missed the local
//! publish fan-out for Follow actions in some orderings. The button therefore
//! showed "Follow" even for already-followed accounts on cold start.
//!
//! # Interest registration
//!
//! `register_follow_state_runtime` (in the crate root) enqueues
//! `ActorCommand::OpenInterest` for `{"kinds":[3],"authors":[<active>]}` on
//! initial registration and on each account change, driving the kernel's
//! cache-serve/store path. The projection closure then reads the event store and
//! the snapshot reflects the canonical kind:3 state immediately.
//!
//! # D-doctrine
//!
//! * **D5** — single source of truth: the event store is the only durable
//!   source of kind:3 follow state. This projection never duplicates it.
//! * **D6** — poisoned mutexes, missing active pubkeys, and empty follow lists
//!   all degrade to `{"follows":[]}` rather than panicking.
//! * **D8** — `snapshot()` holds a read lock for one map lookup, bounded O(n)
//!   in follows. No I/O, no blocking.
//! * **Raw data** — entries carry only the hex pubkey. Presentation layers
//!   format for display (bech32, abbreviation, avatar initials/tint) per
//!   aim.md §2 (NMP is a data framework; backend sends raw protocol data).
//!
//! # Provenance
//!
//! Moved out of `apps/chirp/crates/nmp-app-chirp/src/follow_list.rs` so any Nostr
//! app on top of NMP can wire the NIP-02 follow-list projection without
//! depending on the Chirp app crate (thin-shell rule — Chirp must be a
//! zero-logic delegate to NMP crates).

use nmp_core::slots::ActiveAccountSlot;
use serde::Serialize;

use crate::LatestKind3FollowSet;

/// One entry in the active account's follow list — raw hex pubkey only.
///
/// Presentation layers format for display (bech32 encoding, abbreviation,
/// avatar initials, avatar tint) — aim.md §2.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FollowEntry {
    /// Hex-encoded public key (64 chars).
    pub pubkey: String,
}

impl FollowEntry {
    /// Build a `FollowEntry` from a hex pubkey.
    fn from_hex(pubkey: String) -> Self {
        Self { pubkey }
    }
}

/// Snapshot shape: the active account's follow list.
#[derive(Serialize)]
struct FollowListSnapshotPayload<'a> {
    follows: &'a [FollowEntry],
}

/// Owned snapshot of the active account's follow list — the typed read-model
/// behind both [`FollowListProjection::snapshot_json`] (the authoritative serde
/// shape) and [`FollowListProjection::snapshot`] (the typed-FB sidecar source,
/// ADR-0072). A named field (rather than a bare `Vec`) leaves room to add
/// sibling fields later without a breaking re-shape, and mirrors the
/// `{"follows": […]}` JSON object the host already consumes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FollowListSnapshot {
    /// The active account's follows (empty when no account / no kind:3 yet).
    pub follows: Vec<FollowEntry>,
}

/// Thin read-model over the canonical event-store kind:3 row for the active
/// account's NIP-02 follow list.
///
/// Construct with [`FollowListProjection::new`] passing the shared
/// `active_pubkey` slot and a latest-kind:3 reader sourced from the host's
/// event store. Register the snapshot closure via
/// [`crate::register_follow_state_runtime`] (which also registers the kind:3
/// interest so cache-serve populates the store before the first snapshot tick).
pub struct FollowListProjection {
    /// The active account's hex pubkey. Written by the kernel actor on account
    /// switch (same pattern as `ActiveFollowSet`). `None` means no signed-in
    /// account → snapshot always `{"follows":[]}`.
    active_pubkey: ActiveAccountSlot,
    /// The canonical follow-set source derived from the event store.
    latest_kind3: LatestKind3FollowSet,
}

impl FollowListProjection {
    /// Construct with the kernel's active-account slot and event-store reader.
    #[must_use]
    pub fn new(active_pubkey: ActiveAccountSlot, latest_kind3: LatestKind3FollowSet) -> Self {
        Self {
            active_pubkey,
            latest_kind3,
        }
    }

    /// The active account's follow list as an owned typed snapshot — the
    /// single source of truth behind both [`Self::snapshot_json`] (serde shape)
    /// and the typed-FB sidecar (ADR-0072).
    ///
    /// Reads the active account's latest kind:3 from the event store. An empty
    /// `follows` vector when:
    ///   * No active account (`active_pubkey` slot is `None`).
    ///   * No kind:3 event for the active account has been ingested yet.
    ///   * The active account's kind:3 has zero `p` tags (follows nobody).
    ///   * Any mutex is poisoned (D6).
    #[must_use]
    pub fn snapshot(&self) -> FollowListSnapshot {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => None,
        };

        let follows = match active {
            None => Vec::new(),
            Some(pubkey) => match self.latest_kind3.follows(&pubkey) {
                None => Vec::new(),
                Some(pubkeys) => pubkeys.into_iter().map(FollowEntry::from_hex).collect(),
            },
        };

        FollowListSnapshot { follows }
    }

    /// The snapshot JSON for the `"nmp.follow_list"` projection key.
    ///
    /// Returns the active account's follow list as
    /// `{"follows": [<FollowEntry>, …]}`. Delegates to [`Self::snapshot`] so the
    /// serde shape and the typed-FB sidecar never drift. An empty array under
    /// the same conditions documented on [`Self::snapshot`].
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        let snapshot = self.snapshot();
        serde_json::to_value(FollowListSnapshotPayload {
            follows: &snapshot.follows,
        })
        .unwrap_or_else(|_| serde_json::json!({ "follows": [] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latest_kind3::tests_support::{insert_kind3, reader_with_store};
    use std::sync::{Arc, Mutex};

    fn make_slot(active: Option<&str>) -> ActiveAccountSlot {
        Arc::new(Mutex::new(active.map(|s| s.to_string())))
    }

    fn make_reader_with(pubkey: &str, follows: &[&str]) -> LatestKind3FollowSet {
        let (reader, store) = reader_with_store();
        insert_kind3(&store, pubkey, "eventid01", 100, follows);
        reader
    }

    #[test]
    fn empty_when_no_active_account() {
        let slot = make_slot(None);
        let (reader, _store) = reader_with_store();
        let proj = FollowListProjection::new(slot, reader);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn empty_when_no_kind3_in_store() {
        let slot = make_slot(Some("aabbcc"));
        let (reader, _store) = reader_with_store();
        let proj = FollowListProjection::new(slot, reader);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn follows_surface_from_store() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let slot = make_slot(Some(author));
        let reader = make_reader_with(author, &[followed]);
        let proj = FollowListProjection::new(slot, reader);
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().expect("follows array");
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), followed);
        // FollowEntry carries only the raw hex pubkey — aim.md §2.
        assert!(follows[0].get("npub").is_none());
    }

    #[test]
    fn other_account_store_entry_not_surfaced() {
        // Store has Carol's follows, but the active account is Alice.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let carol = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let reader = make_reader_with(carol, &[followed]);
        let slot = make_slot(Some(alice));
        let proj = FollowListProjection::new(slot, reader);
        let snap = proj.snapshot_json();
        assert_eq!(snap["follows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn account_switch_reads_new_account_from_store() {
        // Both Alice and Bob have entries in the store. Switching the active slot
        // makes the projection reflect the new account immediately.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let alice_follow = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob_follow = "dd11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

        let (reader, store) = reader_with_store();
        insert_kind3(&store, alice, "ev-alice", 100, &[alice_follow]);
        // Bob has NO entry yet (simulates new account before kind:3 arrives).
        let slot = make_slot(Some(alice));
        let proj = FollowListProjection::new(Arc::clone(&slot), reader);

        // Alice is active — her follows appear.
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), alice_follow);

        // Switch to Bob: snapshot must be empty immediately (Bob has no stored kind:3).
        *slot.lock().unwrap() = Some(bob.to_string());
        let snap = proj.snapshot_json();
        assert_eq!(
            snap["follows"].as_array().unwrap().len(),
            0,
            "account switch to new account → empty follows immediately"
        );

        // Bob's kind:3 arrives in the canonical store.
        insert_kind3(&store, bob, "ev-bob", 200, &[bob_follow]);
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(
            follows[0]["pubkey"].as_str().unwrap(),
            bob_follow,
            "after Bob's kind:3 is stored, snapshot reflects his follows"
        );
    }

    #[test]
    fn cleared_follow_set_some_empty_yields_empty_snapshot() {
        // An explicit kind:3 with no p-tags means "follows nobody" — Some([]).
        // The projection must surface an empty list, not None.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let (reader, store) = reader_with_store();
        insert_kind3(&store, author, "ev1", 100, &[]); // no follows
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(slot, reader);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn newer_store_entry_reflected_live() {
        // The store is mutable via the shared Arc; inserting a newer kind:3 is
        // immediately visible in
        // snapshot() without any observer step.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let first = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let second = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let (reader, store) = reader_with_store();
        insert_kind3(&store, author, "ev1", 100, &[first]);
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(slot, reader);
        // First snapshot: only `first`.
        let snap = proj.snapshot();
        assert_eq!(snap.follows.len(), 1);
        assert_eq!(snap.follows[0].pubkey, first);

        // A replacement kind:3 with higher created_at enters the store.
        insert_kind3(&store, author, "ev2", 200, &[second]);
        // Without any observer step the snapshot already reflects the update.
        let snap = proj.snapshot();
        assert_eq!(snap.follows.len(), 1);
        assert_eq!(
            snap.follows[0].pubkey, second,
            "live update from store write is visible without observer fan-out"
        );
    }

    #[test]
    fn multiple_follows_all_surface() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let f1 = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let f2 = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let reader = make_reader_with(author, &[f1, f2]);
        let proj = FollowListProjection::new(make_slot(Some(author)), reader);
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 2);
    }

    #[test]
    fn snapshot_struct_equivalence_for_local_and_external_kind3() {
        // Proves the equivalence lock: a locally-published follow (written by
        // the actor's Follow handler) and an externally-injected
        // kind:3 (e.g. from another device) produce IDENTICAL FollowListSnapshot
        // values when they carry the same follow set. Both paths write the same
        // latest kind:3 store row, so the snapshot is identical.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let (reader, store) = reader_with_store();
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(Arc::clone(&slot), reader);

        // Simulate: local publish writes ev1 (t=100).
        insert_kind3(&store, author, "ev-local", 100, &[bob]);
        let local_snap = proj.snapshot();

        // Simulate: external replacement kind:3 arrives carrying the same follows
        // (same created_at-tie resolved by lex event-id — ev-external > ev-local
        // so it supersedes, but with the same p-tags the snapshot is identical).
        insert_kind3(&store, author, "ev-external", 100, &[bob]);
        let external_snap = proj.snapshot();

        assert_eq!(
            local_snap, external_snap,
            "local follow and external kind:3 replacement with same follows must yield IDENTICAL snapshots"
        );
        assert_eq!(local_snap.follows.len(), 1);
        assert_eq!(local_snap.follows[0].pubkey, bob);
    }
}
