//! `MuteListProjection` — the active account's NIP-51 kind:10000 mute list.
//!
//! # Overview
//!
//! A [`KernelEventObserver`] for kind:10000 (public mute list) events. It
//! accumulates the active account's muted pubkeys (`p` tags) and muted event
//! ids (`e` tags) and exposes them through the substrate-generic
//! [`SuppressionLookup`] trait, which the timeline projection consults when
//! building snapshots.
//!
//! # Why kind:10000 via `KernelEventObserver`
//!
//! Like `FollowListProjection` (kind:3), the mute list is a replaceable event
//! whose data is sig-stripped by the kernel's ingest pipeline before the
//! observer fires. `KernelEventObserver` is the correct seam — the `p`/`e`
//! tags in `KernelEvent.tags` are sufficient; no raw signed bytes are needed.
//!
//! # Public tags only
//!
//! NIP-51 allows private mutes in the NIP-44 encrypted `content` field. This
//! crate deliberately does NOT decrypt that field — decryption requires the
//! active signer and the NIP-44 crypto stack. Public tag parsing is sufficient
//! for the v1 safety requirement and avoids a signer dependency in a read-only
//! projection crate.
//!
//! # Author gate
//!
//! Only the active account's kind:10000 defines suppression. kind:10000 events
//! authored by anyone else (e.g. social-graph contacts surfaced by the WOT
//! bootstrap) are dropped so we never suppress based on a stranger's mute list.
//!
//! # Standing subscription
//!
//! The WOT bootstrap interest pushed by `nmp-wot` includes kind:10000 in its
//! `WOT_BOOTSTRAP_KINDS` list (see `nmp-wot/src/interest.rs`). No separate
//! interest push is needed — the observer will receive the active account's
//! kind:10000 as it arrives.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-core` sees no NIP-51 nouns; it sees `SuppressionLookup`.
//! * **D6** — poisoned mutexes, missing active pubkey, and empty mute lists
//!   all degrade to "suppress nothing" rather than panicking or suppressing
//!   everything.
//! * **D8** — `on_kernel_event` runs synchronously on the actor thread between
//!   relay frames. Work is bounded: one kind filter check, two short mutex
//!   locks, one `p`/`e`-tag scan, one upsert. No I/O, no blocking.
//! * **Raw data** — the projection stores only hex pubkeys and hex event ids.
//!   Presentation layers format for display per aim.md §2.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::KernelEventObserver;
use serde::Serialize;

/// NIP-51 public mute list kind.
const KIND_MUTE_LIST: u32 = 10000;

/// Snapshot shape — the full mute list for diagnostic / export purposes.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MuteListSnapshot {
    /// Muted author pubkeys (hex).
    pub muted_pubkeys: Vec<String>,
    /// Muted event ids (hex).
    pub muted_event_ids: Vec<String>,
}

/// Inner mutable state — the active account's muted pubkeys and event ids.
#[derive(Default)]
struct MuteSet {
    pubkeys: HashSet<String>,
    event_ids: HashSet<String>,
}

/// Accumulates the active account's NIP-51 kind:10000 mute list and exposes
/// a [`SuppressionLookup`] the timeline projection uses to filter cards.
///
/// Construct with a shared `active_pubkey` slot (the same pattern as
/// [`nmp_nip02::FollowListProjection`]). Register the same `Arc` as a
/// [`KernelEventObserver`] against the kernel so kind:10000 events are
/// ingested, and as a [`SuppressionLookup`] that the timeline projection
/// consults when building snapshots.
pub struct MuteListProjection {
    /// The active account's hex pubkey. Written by the FFI on account switch
    /// (same pattern as `nmp17_local_keys` in `DmInboxProjection`). `None`
    /// means no signed-in account → suppress nothing.
    active_pubkey: Arc<Mutex<Option<String>>>,
    /// The active account's current mute set.
    mute_set: Mutex<MuteSet>,
}

impl MuteListProjection {
    /// Construct with a shared `active_pubkey` slot.
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            mute_set: Mutex::new(MuteSet::default()),
        }
    }

    /// Build a snapshot for the `"nmp.mute_list"` projection key.
    ///
    /// Returns the active account's muted pubkeys and event ids as
    /// `{"muted_pubkeys":[…], "muted_event_ids":[…]}`. Both arrays are
    /// empty when no active account or no kind:10000 has arrived yet.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        let snap = self.snapshot();
        serde_json::to_value(snap).unwrap_or_else(|_| {
            serde_json::json!({ "muted_pubkeys": [], "muted_event_ids": [] })
        })
    }

    /// Build a typed snapshot.
    #[must_use]
    pub fn snapshot(&self) -> MuteListSnapshot {
        let Ok(mute_set) = self.mute_set.lock() else {
            return MuteListSnapshot::default();
        };
        let mut muted_pubkeys: Vec<String> = mute_set.pubkeys.iter().cloned().collect();
        let mut muted_event_ids: Vec<String> = mute_set.event_ids.iter().cloned().collect();
        muted_pubkeys.sort_unstable();
        muted_event_ids.sort_unstable();
        MuteListSnapshot {
            muted_pubkeys,
            muted_event_ids,
        }
    }

    /// Number of muted pubkeys currently held. Test-only inspector.
    #[cfg(test)]
    pub(crate) fn muted_pubkey_count(&self) -> usize {
        self.mute_set
            .lock()
            .map(|g| g.pubkeys.len())
            .unwrap_or(0)
    }

    /// Number of muted event ids currently held. Test-only inspector.
    #[cfg(test)]
    pub(crate) fn muted_event_id_count(&self) -> usize {
        self.mute_set
            .lock()
            .map(|g| g.event_ids.len())
            .unwrap_or(0)
    }
}

impl KernelEventObserver for MuteListProjection {
    /// Called by the kernel once per accepted kind:10000 event.
    ///
    /// Gate by `kind == 10000` **and** by author == active pubkey, then
    /// extract all `["p", <pubkey>, …]` and `["e", <event_id>, …]` tags and
    /// store them. Replaceable: a newer kind:10000 from the same author
    /// overwrites the previous entry (the kernel deduplicates via `Replaced`
    /// — this upsert is idempotent). Poisoned mutex → silent no-op (D6).
    ///
    /// # Why the author gate
    ///
    /// `is_suppressed_author` only uses the active account's mute set, so
    /// kind:10000 events authored by anyone else would accumulate as dead
    /// weight. On account switch, the kernel re-fetches kind:10000 via the
    /// WOT bootstrap interest so the new active account's mute list
    /// repopulates on its own.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_MUTE_LIST {
            return;
        }

        // Author gate: skip unless this kind:10000 was authored by the active
        // account. Poisoned mutex or no active account → silent no-op (D6).
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return,
        };
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }

        let pubkeys: HashSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|t| t == "p") {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();

        let event_ids: HashSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|t| t == "e") {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();

        let Ok(mut mute_set) = self.mute_set.lock() else {
            return;
        };
        // Full replacement on every kind:10000 event — the NIP-51 replaceable
        // model means the newest event is the complete canonical list.
        *mute_set = MuteSet { pubkeys, event_ids };
    }
}

impl SuppressionLookup for MuteListProjection {
    /// Returns `true` if `author_pubkey` is in the active account's mute set.
    /// Fails open (returns `false`) on a poisoned mutex (D6).
    fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
        self.mute_set
            .lock()
            .map(|g| g.pubkeys.contains(author_pubkey))
            .unwrap_or(false)
    }

    /// Returns `true` if `event_id` is in the active account's mute set.
    /// Fails open (returns `false`) on a poisoned mutex (D6).
    fn is_suppressed_event(&self, event_id: &str) -> bool {
        self.mute_set
            .lock()
            .map(|g| g.event_ids.contains(event_id))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{EventId, KernelEvent};

    fn projection_for(active: Option<&str>) -> MuteListProjection {
        let slot = Arc::new(Mutex::new(active.map(|s| s.to_string())));
        MuteListProjection::new(slot)
    }

    fn mute_event(author: &str, p_tags: &[&str], e_tags: &[&str]) -> KernelEvent {
        let mut tags: Vec<Vec<String>> = p_tags
            .iter()
            .map(|pk| vec!["p".to_string(), pk.to_string()])
            .collect();
        for eid in e_tags {
            tags.push(vec!["e".to_string(), eid.to_string()]);
        }
        KernelEvent {
            id: EventId::from(
                "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
            ),
            author: author.to_string(),
            kind: 10000,
            created_at: 100,
            tags,
            content: String::new(),
        }
    }

    const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
    const EID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn empty_when_no_active_account() {
        let proj = projection_for(None);
        assert!(!proj.is_suppressed_author(BOB));
        assert!(!proj.is_suppressed_event(EID_A));
    }

    #[test]
    fn empty_when_no_kind10000_received() {
        let proj = projection_for(Some(ALICE));
        assert!(!proj.is_suppressed_author(BOB));
    }

    #[test]
    fn non_kind10000_event_is_ignored() {
        let proj = projection_for(Some(ALICE));
        let mut ev = mute_event(ALICE, &[BOB], &[]);
        ev.kind = 1;
        proj.on_kernel_event(&ev);
        assert!(!proj.is_suppressed_author(BOB));
        assert_eq!(proj.muted_pubkey_count(), 0);
    }

    #[test]
    fn kind10000_for_other_account_is_ignored() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(CAROL, &[BOB], &[]));
        assert!(!proj.is_suppressed_author(BOB));
        assert_eq!(proj.muted_pubkey_count(), 0);
    }

    #[test]
    fn kind10000_for_active_account_suppresses_muted_author() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
        assert!(proj.is_suppressed_author(BOB));
        assert!(!proj.is_suppressed_author(CAROL));
    }

    #[test]
    fn kind10000_for_active_account_suppresses_muted_event_id() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(ALICE, &[], &[EID_A]));
        assert!(proj.is_suppressed_event(EID_A));
        assert!(!proj.is_suppressed_event("other_event_id"));
    }

    #[test]
    fn newer_kind10000_replaces_older_mute_list() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
        assert!(proj.is_suppressed_author(BOB));
        // Replacement: Alice removes Bob from mute list, adds Carol.
        proj.on_kernel_event(&mute_event(ALICE, &[CAROL], &[]));
        assert!(!proj.is_suppressed_author(BOB), "Bob should no longer be muted");
        assert!(proj.is_suppressed_author(CAROL));
    }

    #[test]
    fn multiple_muted_pubkeys_all_suppressed() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(ALICE, &[BOB, CAROL], &[]));
        assert!(proj.is_suppressed_author(BOB));
        assert!(proj.is_suppressed_author(CAROL));
        assert_eq!(proj.muted_pubkey_count(), 2);
    }

    #[test]
    fn snapshot_json_reflects_mute_list() {
        let proj = projection_for(Some(ALICE));
        proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[EID_A]));
        let snap = proj.snapshot();
        assert_eq!(snap.muted_pubkeys, vec![BOB]);
        assert_eq!(snap.muted_event_ids, vec![EID_A]);
    }

    #[test]
    fn account_switch_clears_previous_mute_set() {
        let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
        let proj = MuteListProjection::new(Arc::clone(&slot));
        proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
        assert!(proj.is_suppressed_author(BOB));

        // Account switch: FFI rewrites the active slot to Carol.
        *slot.lock().unwrap() = Some(CAROL.to_string());

        // Carol's kind:10000 arrives — must NOT include Bob.
        proj.on_kernel_event(&mute_event(CAROL, &[], &[]));
        assert!(
            !proj.is_suppressed_author(BOB),
            "after switch, Alice's mute list must not suppress Bob"
        );
    }

    #[test]
    fn no_active_account_drops_all_inserts() {
        let proj = projection_for(None);
        proj.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
        assert_eq!(proj.muted_pubkey_count(), 0);
    }
}
