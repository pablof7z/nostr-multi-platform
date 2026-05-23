//! `FollowListProjection` — the active account's NIP-02 follow list.
//!
//! # Overview
//!
//! A [`KernelEventObserver`] for kind:3 (contact list) events. It accumulates
//! follow lists keyed by the event author and exposes the active account's
//! follows through [`FollowListProjection::snapshot_json`] — the shape a host
//! `register_snapshot_projection` closure returns.
//!
//! # Why kind:3 via `KernelEventObserver`
//!
//! Unlike `DmInboxProjection` (which needs the raw signed event for NIP-44),
//! kind:3 contacts are sig-stripped by the kernel's ingest pipeline before
//! the observer fires. `KernelEventObserver` is the correct seam here — no
//! raw signed bytes are needed; the `p`-tagged pubkeys in `KernelEvent.tags`
//! are sufficient.
//!
//! # Standing subscription
//!
//! The kernel already fetches kind:3 for the active account as part of the
//! `account_profile_interest` (kind:0 + kind:3 + kind:10002). No separate
//! interest push is needed — the observer will receive kind:3 events as they
//! arrive in the kernel's event store.
//!
//! # D-doctrine
//!
//! * **D6** — poisoned mutexes, missing active pubkeys, and empty follow lists
//!   all degrade to `{"follows":[]}` rather than panicking.
//! * **D8** — `on_kernel_event` runs synchronously on the actor thread between
//!   relay frames. Work is bounded: one kind filter check, two short mutex
//!   locks (active-pubkey gate + follows map), one `p`-tag scan, one
//!   `HashMap` insert. No I/O, no blocking.
//! * **Thin-shell** — all display strings (npub, abbreviated form, initials,
//!   colour) are computed here. The Swift shell renders only what it receives.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_nip17::display;
use serde::Serialize;

/// NIP-02 contact list kind.
const KIND_CONTACT_LIST: u32 = 3;

/// One entry in the active account's follow list — pre-formatted for display.
///
/// All display strings are computed in Rust at snapshot time (thin-shell rule).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FollowEntry {
    /// Hex-encoded public key (64 chars).
    pub pubkey: String,
    /// Bech32 `npub1…` encoding. Falls back to raw hex on parse error (D6).
    pub npub: String,
    /// Abbreviated bech32: 10 head chars + `"…"` + 6 tail chars.
    pub short_npub: String,
    /// Two-char uppercase initials for the avatar tile.
    pub avatar_initials: String,
    /// Six-hex deterministic avatar background colour (uppercase, no `#`).
    pub avatar_color: String,
}

impl FollowEntry {
    /// Build a `FollowEntry` from a hex pubkey, computing all display fields.
    fn from_hex(pubkey: String) -> Self {
        let npub = display::to_npub(&pubkey);
        FollowEntry {
            short_npub: display::short_npub(&pubkey),
            avatar_initials: display::avatar_initials(&npub),
            avatar_color: display::avatar_color_hex(&pubkey),
            npub,
            pubkey,
        }
    }
}

/// Snapshot shape: the active account's follow list.
#[derive(Serialize)]
struct FollowListSnapshotPayload<'a> {
    follows: &'a [FollowEntry],
}

/// Accumulates NIP-02 kind:3 contact lists and exposes the active account's
/// follow list as a formatted snapshot.
///
/// Construct with a shared `active_pubkey` slot; the chirp FFI writes the
/// slot on account creation / switch. Register the same `Arc` as a
/// [`KernelEventObserver`] against the kernel so kind:3 events are ingested.
pub struct FollowListProjection {
    /// The active account's hex pubkey. Written by the FFI on account switch
    /// (same pattern as `nmp17_local_keys` in `DmInboxProjection`). `None`
    /// means no signed-in account → snapshot always `{"follows":[]}`.
    active_pubkey: Arc<Mutex<Option<String>>>,
    /// Accumulated follow lists keyed by event author pubkey (hex). The value
    /// is the list of followed pubkeys extracted from `["p", pubkey, …]` tags.
    /// A single author may publish multiple kind:3 events; only the newest is
    /// kept because the kernel routes replaceable events through `Replaced`
    /// (the old entry was superseded before the observer fires).
    follows: Mutex<HashMap<String, Vec<String>>>,
}

impl FollowListProjection {
    /// Construct with a shared `active_pubkey` slot.
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            follows: Mutex::new(HashMap::new()),
        }
    }

    /// The snapshot JSON for the `"nmp.follow_list"` projection key.
    ///
    /// Returns the active account's follow list as
    /// `{"follows": [<FollowEntry>, …]}`. An empty array when:
    ///   * No active account (`active_pubkey` slot is `None`).
    ///   * No kind:3 event for the active account has arrived yet.
    ///   * The active account's kind:3 has zero `p` tags (follows nobody).
    ///   * Any mutex is poisoned (D6).
    /// Number of entries currently held in the follows map. Test-only
    /// inspector for the shadow-storage gate invariant: after the author
    /// guard in `on_kernel_event`, this must be `<= 1` regardless of how
    /// many distinct authors publish kind:3.
    #[cfg(test)]
    fn follows_map_len(&self) -> usize {
        self.follows.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn snapshot_json(&self) -> serde_json::Value {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => None,
        };

        let follows_vec: Vec<FollowEntry> = match active {
            None => Vec::new(),
            Some(pubkey) => {
                let follows_guard = match self.follows.lock() {
                    Ok(g) => g,
                    Err(_) => return serde_json::json!({ "follows": [] }),
                };
                match follows_guard.get(&pubkey) {
                    None => Vec::new(),
                    Some(pubkeys) => pubkeys
                        .iter()
                        .map(|pk| FollowEntry::from_hex(pk.clone()))
                        .collect(),
                }
            }
        };

        serde_json::to_value(FollowListSnapshotPayload {
            follows: &follows_vec,
        })
        .unwrap_or_else(|_| serde_json::json!({ "follows": [] }))
    }
}

impl KernelEventObserver for FollowListProjection {
    /// Called by the kernel once per accepted kind:3 event.
    ///
    /// Gate by `kind == 3` **and** by author == active pubkey, then extract
    /// all `["p", <pubkey>, …]` tags and store them under the event's author.
    /// Replaceable: a newer kind:3 from the same author overwrites the
    /// previous entry (the kernel already deduplicates via `Replaced` — this
    /// upsert is idempotent). Poisoned mutex → silent no-op (D6).
    ///
    /// # Why the author gate
    ///
    /// `snapshot_json` only ever reads the active account's entry, so kind:3
    /// events authored by anyone else (e.g. profiles surfaced in a follow
    /// feed) would accumulate as dead weight — a shadow-storage leak that
    /// scales with the social graph. The kernel already stores the active
    /// account's follow list authoritatively in `seed_contacts`, so any
    /// non-active-author insert would also be a duplicate index.
    ///
    /// On account switch, the kernel re-fetches kind:3 via
    /// `account_profile_interest`, so the new active account's follow list
    /// repopulates on its own.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_CONTACT_LIST {
            return;
        }

        // Author gate: skip unless this kind:3 was authored by the active
        // account. Poisoned mutex or no active account → silent no-op (D6).
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return,
        };
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }

        let followed: Vec<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().map(|t| t == "p").unwrap_or(false) {
                    tag.get(1).cloned()
                } else {
                    None
                }
            })
            .collect();

        let Ok(mut follows) = self.follows.lock() else {
            return;
        };
        // The author gate above guarantees this insert is for the active
        // account. Clear first so stale entries from a previous active
        // account (e.g. after account switch) don't linger — the map's
        // invariant is `len() <= 1`, always the current active account.
        follows.clear();
        follows.insert(event.author.clone(), followed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::KernelEvent;

    fn make_event(author: &str, p_tags: &[&str]) -> KernelEvent {
        let tags: Vec<Vec<String>> = p_tags
            .iter()
            .map(|pk| vec!["p".to_string(), pk.to_string()])
            .collect();
        KernelEvent {
            id: nmp_core::substrate::EventId::from("0000000000000000000000000000000000000000000000000000000000000001".to_string()),
            author: author.to_string(),
            kind: 3,
            created_at: 100,
            tags,
            content: String::new(),
        }
    }

    fn projection_for(active: Option<&str>) -> FollowListProjection {
        let slot = Arc::new(Mutex::new(active.map(|s| s.to_string())));
        FollowListProjection::new(slot)
    }

    #[test]
    fn empty_when_no_active_account() {
        let proj = projection_for(None);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn empty_when_no_kind3_received() {
        let proj = projection_for(Some("aabbcc"));
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn non_kind3_event_is_ignored() {
        let proj = projection_for(Some("aabbcc"));
        let mut ev = make_event("aabbcc", &["ddeeff"]);
        ev.kind = 1; // kind:1 note — must not update follows
        proj.on_kernel_event(&ev);
        let snap = proj.snapshot_json();
        assert_eq!(snap, serde_json::json!({ "follows": [] }));
    }

    #[test]
    fn kind3_for_active_account_surfaces_in_snapshot() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let proj = projection_for(Some(author));

        proj.on_kernel_event(&make_event(author, &[followed]));

        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().expect("follows array");
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), followed);
        // npub must start with npub1
        assert!(
            follows[0]["npub"].as_str().unwrap().starts_with("npub1"),
            "npub must be bech32 encoded"
        );
        // short_npub must contain ellipsis
        assert!(
            follows[0]["short_npub"].as_str().unwrap().contains('…'),
            "short_npub must be abbreviated"
        );
    }

    #[test]
    fn kind3_for_other_account_is_not_surfaced() {
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let carol = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

        // Active account is Alice; Carol's kind:3 arrives.
        let proj = projection_for(Some(alice));
        proj.on_kernel_event(&make_event(carol, &[followed]));

        let snap = proj.snapshot_json();
        assert_eq!(snap["follows"].as_array().unwrap().len(), 0);
        // Shadow-storage gate: Carol's kind:3 must not have been stored at
        // all — the projection only keeps the active account's list.
        assert_eq!(proj.follows_map_len(), 0);
    }

    #[test]
    fn many_non_active_authors_do_not_grow_map() {
        // Regression: prior to the author gate, every kind:3 from every
        // author in the follow feed was inserted, growing the map without
        // bound. With the gate, the map must hold at most one entry — the
        // active account's list.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let proj = projection_for(Some(alice));

        // 50 distinct non-active authors publish kind:3.
        for i in 0..50u8 {
            let mut author = String::with_capacity(64);
            // Two-hex byte prefix that varies per iteration, plus a fixed
            // 62-hex-char tail — produces 50 distinct, valid-looking hex pubkeys
            // that are all different from `alice`.
            author.push_str(&format!("{:02x}", i));
            author.push_str("99887766554433221100ffeeddccbbaa99887766554433221100ffeeddccbb");
            assert_ne!(author, alice);
            proj.on_kernel_event(&make_event(&author, &["deadbeef"]));
        }

        // Active account never published — map must remain empty.
        assert_eq!(
            proj.follows_map_len(),
            0,
            "non-active authors must not be stored"
        );

        // Now the active account publishes its kind:3 → exactly one entry.
        let followed = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        proj.on_kernel_event(&make_event(alice, &[followed]));
        assert!(
            proj.follows_map_len() <= 1,
            "map must hold at most the active account's entry, got {}",
            proj.follows_map_len()
        );
        assert_eq!(proj.follows_map_len(), 1);

        // And the snapshot reflects Alice's list.
        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), followed);
    }

    #[test]
    fn account_switch_does_not_strand_stale_entry() {
        // Alice signs in, her kind:3 lands → map = {Alice}. User switches
        // to Bob (FFI writes the active slot). Bob's kind:3 lands → the
        // map must hold ONLY Bob; Alice's entry must be cleared. This is
        // the `<= 1` invariant across account switches.
        let alice = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let alice_follow = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let bob_follow = "dd11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

        let slot = Arc::new(Mutex::new(Some(alice.to_string())));
        let proj = FollowListProjection::new(Arc::clone(&slot));

        // Alice's kind:3 arrives.
        proj.on_kernel_event(&make_event(alice, &[alice_follow]));
        assert_eq!(proj.follows_map_len(), 1);

        // Account switch: FFI rewrites the active slot to Bob.
        *slot.lock().unwrap() = Some(bob.to_string());

        // Bob's kind:3 arrives — Alice's stale entry must be cleared.
        proj.on_kernel_event(&make_event(bob, &[bob_follow]));
        assert_eq!(
            proj.follows_map_len(),
            1,
            "after switch, map must hold only the new active account"
        );

        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), bob_follow);
    }

    #[test]
    fn no_active_account_drops_all_inserts() {
        // With no active pubkey set, even an event that "could" have been
        // ours (if we later signed in as that author) is dropped. This is
        // the correct semantics: the kernel re-fetches kind:3 on sign-in.
        let proj = projection_for(None);
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        proj.on_kernel_event(&make_event(author, &["deadbeef"]));
        assert_eq!(proj.follows_map_len(), 0);
    }

    #[test]
    fn newer_kind3_replaces_older_follow_list() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let first = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let second = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let proj = projection_for(Some(author));

        proj.on_kernel_event(&make_event(author, &[first]));
        // A replacement kind:3 with a different follow list.
        proj.on_kernel_event(&make_event(author, &[second]));

        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 1);
        assert_eq!(follows[0]["pubkey"].as_str().unwrap(), second);
    }

    #[test]
    fn multiple_follows_all_surface() {
        let author = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let f1 = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
        let f2 = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
        let proj = projection_for(Some(author));

        proj.on_kernel_event(&make_event(author, &[f1, f2]));

        let snap = proj.snapshot_json();
        let follows = snap["follows"].as_array().unwrap();
        assert_eq!(follows.len(), 2);
    }
}
