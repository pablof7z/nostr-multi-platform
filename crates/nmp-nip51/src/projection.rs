//! `MuteListProjection` — the active account's NIP-51 kind:10000 mute list.
//!
//! # Overview
//!
//! A [`ObservedProjectionSink`] for kind:10000 (public mute list) events. It
//! accumulates the active account's muted pubkeys (`p` tags) and muted event
//! ids (`e` tags) and exposes them through the substrate-generic
//! [`SuppressionLookup`] trait, which feed/timeline projections consult when
//! applying active-account suppression.
//!
//! # Why kind:10000 via `ObservedProjectionSink`
//!
//! Like `FollowListProjection` (kind:3), the mute list is a replaceable event
//! whose data is sig-stripped by the kernel's ingest pipeline before the
//! observer fires. `ObservedProjectionSink` is the correct seam — the `p`/`e`
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
//! # Account-switch safety — read-time owner gate
//!
//! The `MuteSet` stores an `owner_pubkey` alongside the muted entries — the
//! hex pubkey of the account whose kind:10000 populated the set. The
//! `SuppressionLookup` read path (`is_suppressed_author`, `is_suppressed_event`)
//! re-reads the live `active_pubkey` slot and **compares it against
//! `owner_pubkey`**. If they differ (account was switched between the write and
//! the read), the stale set is invisible and the methods return `false`.
//!
//! This mirrors the pattern used by `FollowListProjection` (nmp-nip02): gate
//! reads on the live active slot rather than on an explicit clear call. The
//! kernel writes the slot on every account switch, so no additional wiring at
//! the composition root is required — the fix is self-contained and
//! unconditionally correct in production.
//!
//! # Standing subscription
//!
//! The `MuteRuntimeController` (see
//! `crates/nmp-defaults/src/runtimes/mute_runtime.rs`) pushes a
//! `active_mute_list_interest(pubkey)` on sign-in so the kernel has a live
//! `authors=[active_pubkey] / kinds=[10000]` subscription. No separate
//! interest push is needed in this crate — wiring is the host's
//! responsibility via the runtime controller.
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

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_MUTE_LIST;
use serde::Serialize;

// --- Canonical kind:10000 tag parser (shared with nmp-wot) ------------------

/// `true` when `s` is a 64-character ASCII hex string.
///
/// Used to validate pubkeys (`p` tags) and event ids (`e` tags) before
/// inserting them into the mute set. Invalid entries are silently dropped
/// (D6 — degrade gracefully, never panic or admit garbage).
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Extract muted pubkeys from kind:10000 `["p", <pubkey>]` tags.
///
/// This is the **canonical** parser for kind:10000 `p` tags, shared by two
/// distinct consumers:
///
/// - [`MuteListProjection`] — **hard timeline suppression**: hide cards whose
///   author the active account has muted.
/// - `nmp-wot::WotGraph::ingest_mute_list` — **soft trust scoring**: subtract
///   `SELF_MUTE_SCORE` / `FOLLOWED_MUTE_SCORE` from a candidate's WoT rank.
///
/// A single canonical parser (GitHub issue #964 consolidation) guarantees
/// both consumers ingest the same pubkey set from the same event. `nmp-wot`
/// takes a legal Layer-4 sibling dependency on `nmp-nip51` for this function
/// rather than maintaining its own duplicate `p`-tag scanner.
///
/// Each extracted value is validated as a 64-character ASCII hex string;
/// non-hex, too-short, or too-long values are silently dropped (D6).
#[must_use]
pub fn mute_pubkeys_from_tags(tags: &[Vec<String>]) -> BTreeSet<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().is_some_and(|t| t == "p") {
                tag.get(1).filter(|v| is_hex64(v)).cloned()
            } else {
                None
            }
        })
        .collect()
}

/// Built-in `nmp-feed::ListId` value for treating the active account's public
/// mute-list `p` tags as a reduced pubkey source.
///
/// The type itself stays in `nmp-feed`; this crate only owns the stable string
/// value so apps and the default compiler can name the NIP-51 source without
/// adding a NIP-51 enum variant to core/planner.
pub const ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID: &str = "nmp.nip51.active_mute_list.pubkeys";

/// Snapshot shape — the full mute list for diagnostic / export purposes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MuteListSnapshot {
    /// Muted author pubkeys (hex).
    pub muted_pubkeys: Vec<String>,
    /// Muted event ids (hex).
    pub muted_event_ids: Vec<String>,
}

/// Inner mutable state — the active account's muted pubkeys and event ids,
/// stamped with the pubkey of the account that produced the set.
///
/// The `owner_pubkey` is compared against the live `active_pubkey` slot on
/// every read: if they differ the set is treated as empty (account-switch
/// safety — see module doc).
#[derive(Default, Eq, PartialEq)]
struct MuteSet {
    /// Hex pubkey of the account whose kind:10000 populated this set.
    /// `None` means the set has never been populated (initial state).
    owner_pubkey: Option<String>,
    pubkeys: HashSet<String>,
    event_ids: HashSet<String>,
}

/// Accumulates the active account's NIP-51 kind:10000 mute list and exposes
/// a [`SuppressionLookup`] feed/timeline projections use to filter cards.
///
/// Construct with a shared `active_pubkey` slot (the same pattern as
/// [`nmp_nip02::FollowListProjection`]). Register the same `Arc` as a
/// [`ObservedProjectionSink`] against the kernel so kind:10000 events are
/// ingested, and as a [`SuppressionLookup`] that the timeline projection
/// consults when building snapshots.
pub struct MuteListProjection {
    /// The active account's hex pubkey. Written by the FFI on account switch
    /// (same pattern as `nmp17_local_keys` in `DmInboxProjection`). `None`
    /// means no signed-in account → suppress nothing.
    active_pubkey: Arc<Mutex<Option<String>>>,
    /// The active account's current mute set.
    mute_set: Mutex<MuteSet>,
    /// Reactive callbacks fired when the active account's mute set changes.
    on_change: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl MuteListProjection {
    /// Construct with a shared `active_pubkey` slot.
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            mute_set: Mutex::new(MuteSet::default()),
            on_change: Mutex::new(Vec::new()),
        }
    }

    /// Register a callback fired after the active account's mute set changes.
    pub fn on_change(&self, callback: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut callbacks) = self.on_change.lock() {
            callbacks.push(callback);
        }
    }

    /// Notify dependent readers that the active account changed.
    ///
    /// Read paths are already owner-gated by the active-account slot. This
    /// method exists so feed sessions can withdraw the prior account's derived
    /// author interests and reset their visible rows immediately.
    pub fn notify_account_changed(&self) {
        self.notify_changed();
    }

    /// Build a snapshot for the `"nmp.mute_list"` projection key.
    ///
    /// Returns the active account's muted pubkeys and event ids as
    /// `{"muted_pubkeys":[…], "muted_event_ids":[…]}`. Both arrays are
    /// empty when no active account or no kind:10000 has arrived yet.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        let snap = self.snapshot();
        serde_json::to_value(snap)
            .unwrap_or_else(|_| serde_json::json!({ "muted_pubkeys": [], "muted_event_ids": [] }))
    }

    /// Build a typed snapshot.
    ///
    /// Returns the active account's muted pubkeys and event ids. Returns an
    /// empty snapshot when no active account, no kind:10000 has arrived yet,
    /// or the stored set belongs to a different (stale) account (D6).
    #[must_use]
    pub fn snapshot(&self) -> MuteListSnapshot {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return MuteListSnapshot::default(),
        };
        let Ok(mute_set) = self.mute_set.lock() else {
            return MuteListSnapshot::default();
        };
        // Owner gate: only return data when the set belongs to the current
        // active account. On account switch the set is stale until the new
        // account's kind:10000 arrives and overwrites it.
        if mute_set.owner_pubkey.as_deref() != active.as_deref() {
            return MuteListSnapshot::default();
        }
        let mut muted_pubkeys: Vec<String> = mute_set.pubkeys.iter().cloned().collect();
        let mut muted_event_ids: Vec<String> = mute_set.event_ids.iter().cloned().collect();
        muted_pubkeys.sort_unstable();
        muted_event_ids.sort_unstable();
        MuteListSnapshot {
            muted_pubkeys,
            muted_event_ids,
        }
    }

    /// Public mute-list `p` tags as a pubkey-set source.
    ///
    /// Returns empty when there is no active account, the stored list belongs to
    /// another account, the list has not arrived, or a lock is poisoned. This is
    /// fail-closed for reduced-source feeds: no members means no derived author
    /// timeline, never a wildcard acquisition.
    #[must_use]
    pub fn muted_pubkeys(&self) -> BTreeSet<String> {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return BTreeSet::new(),
        };
        let Ok(mute_set) = self.mute_set.lock() else {
            return BTreeSet::new();
        };
        if mute_set.owner_pubkey.as_deref() != active.as_deref() {
            return BTreeSet::new();
        }
        mute_set.pubkeys.iter().cloned().collect()
    }

    /// Number of muted pubkeys currently held. Test-only inspector.
    #[cfg(test)]
    pub(crate) fn muted_pubkey_count(&self) -> usize {
        self.mute_set.lock().map(|g| g.pubkeys.len()).unwrap_or(0)
    }

    fn notify_changed(&self) {
        if let Ok(callbacks) = self.on_change.lock() {
            for callback in callbacks.iter() {
                callback();
            }
        }
    }
}

impl ObservedProjectionSink for MuteListProjection {
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
    /// weight. On account switch, `MuteRuntimeController` withdraws the old
    /// interest and pushes a new one so the new active account's mute list
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

        // Use the canonical shared parser (also used by nmp-wot for trust
        // scoring) so both consumers ingest identical pubkey sets (#964).
        let pubkeys: HashSet<String> = mute_pubkeys_from_tags(&event.tags).into_iter().collect();

        let event_ids: HashSet<String> = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().is_some_and(|t| t == "e") {
                    tag.get(1).filter(|v| is_hex64(v)).cloned()
                } else {
                    None
                }
            })
            .collect();

        let changed = {
            let Ok(mut mute_set) = self.mute_set.lock() else {
                return;
            };
            let next = MuteSet {
                owner_pubkey: Some(event.author.clone()),
                pubkeys,
                event_ids,
            };
            if *mute_set == next {
                false
            } else {
                *mute_set = next;
                true
            }
        };
        if changed {
            self.notify_changed();
        }
    }
}

impl SuppressionLookup for MuteListProjection {
    /// Returns `true` if `author_pubkey` is in the active account's mute set.
    ///
    /// Reads the live `active_pubkey` slot and compares it against the set's
    /// `owner_pubkey`. If they differ (e.g. after an account switch before the
    /// new account's kind:10000 arrives) returns `false` — the stale set from
    /// the prior account is invisible. Fails open (returns `false`) on a
    /// poisoned mutex (D6).
    fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return false,
        };
        let active = match active {
            Some(pk) => pk,
            None => return false,
        };
        self.mute_set
            .lock()
            .map(|g| {
                g.owner_pubkey.as_deref() == Some(active.as_str())
                    && g.pubkeys.contains(author_pubkey)
            })
            .unwrap_or(false)
    }

    /// Returns `true` if `event_id` is in the active account's mute set.
    ///
    /// Applies the same read-time owner gate as [`Self::is_suppressed_author`].
    /// Fails open (returns `false`) on a poisoned mutex (D6).
    fn is_suppressed_event(&self, event_id: &str) -> bool {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return false,
        };
        let active = match active {
            Some(pk) => pk,
            None => return false,
        };
        self.mute_set
            .lock()
            .map(|g| {
                g.owner_pubkey.as_deref() == Some(active.as_str()) && g.event_ids.contains(event_id)
            })
            .unwrap_or(false)
    }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
