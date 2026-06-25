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
//! The canonical kind:3 follow state lives in the shared
//! [`nmp_core::substrate::ContactsLookup`] (written by `nmp_nip01::Kind3Parser`
//! on every ingest, including local publishes and cache-serves). This projection
//! is a **thin read-model** over that single source of truth: `snapshot()` calls
//! `contacts_lookup.follows(active_pubkey)` and maps the raw hex pubkeys to
//! [`FollowEntry`] values. No secondary `HashMap` is maintained.
//!
//! # Why the old `KernelEventObserver` approach was broken
//!
//! The prior design kept an observer-local `HashMap` populated only by
//! `KernelEventObserver::on_kernel_event`. This missed the startup cache-serve
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
//! cache-serve → `Kind3Parser` → `ContactsLookup` pipeline. The projection
//! closure then reads `contacts_lookup.follows` and the snapshot reflects the
//! cached state immediately — no observer lag.
//!
//! # D-doctrine
//!
//! * **D5** — single source of truth: `ContactsLookup` is the ONLY store of
//!   the parsed follow set. This projection never duplicates it.
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

use std::sync::Arc;

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::ContactsLookup;
use serde::Serialize;

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
/// ADR-0037). A named field (rather than a bare `Vec`) leaves room to add
/// sibling fields later without a breaking re-shape, and mirrors the
/// `{"follows": […]}` JSON object the host already consumes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FollowListSnapshot {
    /// The active account's follows (empty when no account / no kind:3 yet).
    pub follows: Vec<FollowEntry>,
}

/// Thin read-model over the canonical [`ContactsLookup`] for the active
/// account's NIP-02 follow list.
///
/// Construct with [`FollowListProjection::new`] passing the shared
/// `active_pubkey` slot and the `contacts_lookup` the composition root
/// installed (the same `Arc<nmp_nip01::ContactsCache>` the `Kind3Parser` writes
/// to). Register the snapshot closure via [`crate::register_follow_state_runtime`]
/// (which also registers the kind:3 interest so cache-serve populates the
/// lookup before the first snapshot tick).
pub struct FollowListProjection {
    /// The active account's hex pubkey. Written by the kernel actor on account
    /// switch (same pattern as `ActiveFollowSet`). `None` means no signed-in
    /// account → snapshot always `{"follows":[]}`.
    active_pubkey: ActiveAccountSlot,
    /// The canonical follow-set source. Written by `nmp_nip01::Kind3Parser` on
    /// every kind:3 ingest (cache-serve and relay delivery). This projection
    /// is a pure read over it — no secondary storage.
    contacts_lookup: Arc<dyn ContactsLookup>,
}

impl FollowListProjection {
    /// Construct with the kernel's active-account slot and the shared contacts
    /// lookup.
    ///
    /// Both `active_pubkey` and `contacts_lookup` must be the SAME `Arc`s the
    /// kernel and the `Kind3Parser` already hold — the composition root
    /// (e.g. `register_follow_state_runtime`) sources them from the app.
    #[must_use]
    pub fn new(active_pubkey: ActiveAccountSlot, contacts_lookup: Arc<dyn ContactsLookup>) -> Self {
        Self {
            active_pubkey,
            contacts_lookup,
        }
    }

    /// The active account's follow list as an owned typed snapshot — the
    /// single source of truth behind both [`Self::snapshot_json`] (serde shape)
    /// and the typed-FB sidecar (ADR-0037).
    ///
    /// Reads `contacts_lookup.follows(active_pubkey)` — the canonical parsed
    /// state written by `Kind3Parser`. An empty `follows` vector when:
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
            Some(pubkey) => match self.contacts_lookup.follows(&pubkey) {
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
    use nmp_core::substrate::TestContactsCache;
    use std::sync::{Arc, Mutex};

    fn make_slot(active: Option<&str>) -> ActiveAccountSlot {
        Arc::new(Mutex::new(active.map(|s| s.to_string())))
    }

    fn make_cache_with(pubkey: &str, follows: &[&str]) -> Arc<TestContactsCache> {
        let cache = Arc::new(TestContactsCache::new());
        let tags: Vec<Vec<String>> = follows
            .iter()
            .map(|pk| vec!["p".to_string(), pk.to_string()])
            .collect();
        cache.ingest_kind3(pubkey, "eventid01", 100, &tags);
        cache
    }

    #[test]
    fn empty_when_no_active_account() {
        let slot = make_slot(None);
        let cache = Arc::new(TestContactsCache::new());
        let proj = FollowListProjection::new(slot, cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn empty_when_no_kind3_in_cache() {
        let slot = make_slot(Some("aabbcc"));
        let cache = Arc::new(TestContactsCache::new()); // nothing cached
        let proj = FollowListProjection::new(slot, cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn follows_surface_from_cache() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let slot = make_slot(Some(author));
        let cache = make_cache_with(author, &[followed]);
        let proj = FollowListProjection::new(slot, cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().expect("follows array");
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), followed);
        // FollowEntry carries only the raw hex pubkey — aim.md §2.
        assert!(follows[0].get("npub").is_none());
    }

    #[test]
    fn other_account_cache_entry_not_surfaced() {
        // Cache has Carol's follows, but the active account is Alice.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let carol = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let cache = make_cache_with(carol, &[followed]);
        let slot = make_slot(Some(alice));
        let proj = FollowListProjection::new(slot, cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        assert_eq!(snap["follows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn account_switch_reads_new_account_from_cache() {
        // Both Alice and Bob have entries in the cache. Switching the active slot
        // makes the projection reflect the new account immediately.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let alice_follow = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob_follow = "dd11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

        let cache = Arc::new(TestContactsCache::new());
        {
            let tags: Vec<Vec<String>> = vec![vec!["p".to_string(), alice_follow.to_string()]];
            cache.ingest_kind3(alice, "ev-alice", 100, &tags);
        }
        // Bob has NO entry yet (simulates new account before kind:3 arrives).
        let slot = make_slot(Some(alice));
        let proj = FollowListProjection::new(
            Arc::clone(&slot),
            Arc::clone(&cache) as Arc<dyn ContactsLookup>,
        );

        // Alice is active — her follows appear.
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), alice_follow);

        // Switch to Bob: snapshot must be empty immediately (Bob has no cached kind:3).
        *slot.lock().unwrap() = Some(bob.to_string());
        let snap = proj.snapshot_json();
        assert_eq!(
            snap["follows"].as_array().unwrap().len(),
            0,
            "account switch to new account → empty follows immediately"
        );

        // Bob's kind:3 arrives (written into the shared cache by Kind3Parser).
        {
            let tags: Vec<Vec<String>> = vec![vec!["p".to_string(), bob_follow.to_string()]];
            cache.ingest_kind3(bob, "ev-bob", 200, &tags);
        }
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(
            follows[0]["pubkey"].as_str().unwrap(),
            bob_follow,
            "after Bob's kind:3 is cached, snapshot reflects his follows"
        );
    }

    #[test]
    fn cleared_follow_set_some_empty_yields_empty_snapshot() {
        // An explicit kind:3 with no p-tags means "follows nobody" — Some([]).
        // The projection must surface an empty list, not None.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let cache = Arc::new(TestContactsCache::new());
        cache.ingest_kind3(author, "ev1", 100, &[]); // no follows
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(slot, cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn newer_cache_entry_reflected_live() {
        // The cache is mutable via the shared Arc; updating it (simulating
        // Kind3Parser ingesting a newer kind:3) is immediately visible in
        // snapshot() without any observer step.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let first = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let second = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let cache = Arc::new(TestContactsCache::new());
        {
            let tags = vec![vec!["p".to_string(), first.to_string()]];
            cache.ingest_kind3(author, "ev1", 100, &tags);
        }
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(slot, Arc::clone(&cache) as Arc<dyn ContactsLookup>);
        // First snapshot: only `first`.
        let snap = proj.snapshot();
        assert_eq!(snap.follows.len(), 1);
        assert_eq!(snap.follows[0].pubkey, first);

        // Kind3Parser ingests a replacement kind:3 (higher created_at).
        {
            let tags = vec![vec!["p".to_string(), second.to_string()]];
            cache.ingest_kind3(author, "ev2", 200, &tags);
        }
        // Without any observer step the snapshot already reflects the update.
        let snap = proj.snapshot();
        assert_eq!(snap.follows.len(), 1);
        assert_eq!(
            snap.follows[0].pubkey, second,
            "live update from cache write is visible without observer fan-out"
        );
    }

    #[test]
    fn multiple_follows_all_surface() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let f1 = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let f2 = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let cache = Arc::new(TestContactsCache::new());
        {
            let tags = vec![
                vec!["p".to_string(), f1.to_string()],
                vec!["p".to_string(), f2.to_string()],
            ];
            cache.ingest_kind3(author, "ev1", 100, &tags);
        }
        let proj =
            FollowListProjection::new(make_slot(Some(author)), cache as Arc<dyn ContactsLookup>);
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 2);
    }

    #[test]
    fn snapshot_struct_equivalence_for_local_and_external_kind3() {
        // Proves the equivalence lock: a locally-published follow (written by
        // the actor's Follow handler via Kind3Parser) and an externally-injected
        // kind:3 (e.g. from another device) produce IDENTICAL FollowListSnapshot
        // values when they carry the same follow set. Both paths write the same
        // ContactsLookup, so the snapshot is identical.
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let cache = Arc::new(TestContactsCache::new());
        let slot = make_slot(Some(author));
        let proj = FollowListProjection::new(
            Arc::clone(&slot),
            Arc::clone(&cache) as Arc<dyn ContactsLookup>,
        );

        // Simulate: local publish writes ev1 (t=100).
        {
            let tags = vec![vec!["p".to_string(), bob.to_string()]];
            cache.ingest_kind3(author, "ev-local", 100, &tags);
        }
        let local_snap = proj.snapshot();

        // Simulate: external replacement kind:3 arrives carrying the same follows
        // (same created_at-tie resolved by lex event-id — ev-external > ev-local
        // so it supersedes, but with the same p-tags the snapshot is identical).
        {
            let tags = vec![vec!["p".to_string(), bob.to_string()]];
            cache.ingest_kind3(author, "ev-external", 100, &tags);
        }
        let external_snap = proj.snapshot();

        assert_eq!(
            local_snap, external_snap,
            "local follow and external kind:3 replacement with same follows must yield IDENTICAL snapshots"
        );
        assert_eq!(local_snap.follows.len(), 1);
        assert_eq!(local_snap.follows[0].pubkey, bob);
    }
}
