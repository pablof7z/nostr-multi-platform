//! `PeopleListProjection` — the active account's NIP-51 kind:30000 follow sets
//! (people lists), keyed by their `d`-tag identifier (#1740 step 3).
//!
//! # Overview
//!
//! A [`ObservedProjectionSink`] for kind:30000 (NIP-51 follow set / people list)
//! events. An addressable, parameterized-replaceable event identified by its
//! `d`-tag: one author may own MANY follow sets, one per `d` value. Each set's
//! `["p", <pubkey>]` tags are the list's MEMBERS (subjects, not recipients).
//! The perspective compiler's `ListMembers { list }` scope resolves a list id to
//! its member pubkeys through this projection.
//!
//! This is the canonical single home for NIP-51 follow-set member resolution
//! (D4): sibling to [`crate::MuteListProjection`] (kind:10000) and
//! [`crate::bookmarks::BookmarkListProjection`] (kind:10003), following the same
//! shared-`active_pubkey`-slot + author-gate + read-time owner-gate pattern.
//!
//! # Public tags only
//!
//! NIP-51 allows private list members in the NIP-44 encrypted `content` field.
//! This crate deliberately does NOT decrypt that field (it would require the
//! active signer + NIP-44 crypto in a read-only projection crate). Public `p`
//! tags are sufficient for the v1 perspective-compiler requirement.
//!
//! # Author gate + account-switch safety
//!
//! Only the active account's kind:30000 events populate the projection (the
//! perspective compiler resolves the *active viewer's* lists). On account switch
//! the kernel writes the active-pubkey slot; the read path gates members on the
//! owner that populated them, so a stale list from the prior account is
//! invisible.
//!
//! # D-doctrine
//!
//! * **D0** — `nmp-core` sees only `ObservedProjectionSink`; the NIP-51 noun stays
//!   in this crate.
//! * **D6** — poisoned mutexes, missing active pubkey, an absent list, and empty
//!   lists all degrade to "no members" rather than panicking. Fail-closed: an
//!   unknown list resolves to the empty set (admit nobody downstream).
//! * **D8** — `on_kernel_event` is bounded: one kind check, one author gate, one
//!   `d`-tag read, one `p`-tag scan, one upsert. No I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_kinds::KIND_FOLLOW_SET;
use serde::Serialize;

/// Snapshot of one follow set's members (diagnostic / export).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PeopleListSnapshot {
    /// The list's `d`-tag identifier.
    pub list_id: String,
    /// The member pubkeys (lowercase hex), sorted.
    pub members: Vec<String>,
}

/// The active account's kind:30000 follow sets, keyed by `d`-tag.
///
/// Construct with the shared `active_pubkey` slot (the same pattern as
/// [`crate::MuteListProjection`]). Register the same `Arc` as a
/// [`ObservedProjectionSink`] so kind:30000 events are ingested.
pub struct PeopleListProjection {
    /// The active account's hex pubkey, written by the FFI on account switch.
    active_pubkey: Arc<Mutex<Option<String>>>,
    /// `d`-tag → member pubkeys, plus the owner that populated each list.
    lists: Mutex<ListStore>,
    /// Reactive callbacks fired when a list the active account owns changes.
    on_change: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

/// One follow set's members plus the `created_at` of the kind:30000 event that
/// produced them — so an older replaceable event never overwrites a newer one
/// (the addressable-replaceable newest-wins contract; NIT #1740 step 3).
#[derive(Default)]
struct ListEntry {
    members: BTreeSet<String>,
    created_at: u64,
}

/// The owned list store: the owner pubkey (for the read-time gate) plus the
/// per-`d`-tag member sets (each with its source event's `created_at`).
#[derive(Default)]
struct ListStore {
    /// The account whose kind:30000 events populated `lists`. On account switch
    /// the store is stale until the new account's lists arrive.
    owner_pubkey: Option<String>,
    /// `d`-tag → list entry (members + source `created_at`).
    lists: BTreeMap<String, ListEntry>,
}

impl PeopleListProjection {
    /// Construct with a shared `active_pubkey` slot.
    #[must_use]
    pub fn new(active_pubkey: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            active_pubkey,
            lists: Mutex::new(ListStore::default()),
            on_change: Mutex::new(Vec::new()),
        }
    }

    /// Register a callback fired after a list the active account owns changes.
    pub fn on_change(&self, callback: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut callbacks) = self.on_change.lock() {
            callbacks.push(callback);
        }
    }

    /// Notify the projection that the active account changed.
    ///
    /// Membership reads are already owner-gated by the active-account slot, so
    /// this does not need to rewrite stored lists. It exists to wake dependent
    /// feed sessions so they withdraw the prior account's member interests,
    /// reset visible rows, and re-acquire the new account's source list.
    pub fn notify_account_changed(&self) {
        self.notify_changed();
    }

    /// The members of the active account's follow set identified by `list_id`
    /// (its `d`-tag), sorted lowercase hex.
    ///
    /// Returns the EMPTY set (fail-closed, D6) when: no active account, the
    /// store belongs to a different (stale) account, the list has not arrived,
    /// or a lock is poisoned. The perspective compiler treats an empty member
    /// set as "admit nobody".
    #[must_use]
    pub fn members(&self, list_id: &str) -> BTreeSet<String> {
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return BTreeSet::new(),
        };
        let Ok(store) = self.lists.lock() else {
            return BTreeSet::new();
        };
        // Owner gate: only serve lists owned by the current active account.
        if store.owner_pubkey.as_deref() != active.as_deref() {
            return BTreeSet::new();
        }
        store
            .lists
            .get(list_id)
            .map(|entry| entry.members.clone())
            .unwrap_or_default()
    }

    /// A diagnostic snapshot of one list.
    #[must_use]
    pub fn snapshot(&self, list_id: &str) -> PeopleListSnapshot {
        PeopleListSnapshot {
            list_id: list_id.to_string(),
            members: self.members(list_id).into_iter().collect(),
        }
    }

    fn notify_changed(&self) {
        if let Ok(callbacks) = self.on_change.lock() {
            for callback in callbacks.iter() {
                callback();
            }
        }
    }
}

impl ObservedProjectionSink for PeopleListProjection {
    /// Called by the kernel once per accepted kind:30000 event.
    ///
    /// Gate by `kind == 30000` AND author == active pubkey, read the `d`-tag,
    /// extract the `["p", <pubkey>]` members, and upsert the list. A newer
    /// kind:30000 with the same `(author, d)` replaces the previous members
    /// (addressable-replaceable). Poisoned mutex / no active account → silent
    /// no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != KIND_FOLLOW_SET {
            return;
        }
        let active = match self.active_pubkey.lock() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => return,
        };
        if active.as_deref() != Some(event.author.as_str()) {
            return;
        }

        // The `d`-tag identifier (empty string when absent — a default set).
        let list_id = event
            .tags
            .iter()
            .find(|tag| tag.first().is_some_and(|t| t == "d"))
            .and_then(|tag| tag.get(1).cloned())
            .unwrap_or_default();

        let members: BTreeSet<String> = event
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

        let changed = {
            let Ok(mut store) = self.lists.lock() else {
                return;
            };
            // Account-switch reset: a different owner wipes the prior account's
            // lists before recording the new owner's set.
            if store.owner_pubkey.as_deref() != Some(event.author.as_str()) {
                store.owner_pubkey = Some(event.author.clone());
                store.lists.clear();
            }
            match store.lists.get(&list_id) {
                // Newest-wins (addressable-replaceable): an OLDER event for the
                // same `(owner, d)` is ignored — it must not overwrite a newer
                // member set. Matches the sibling NIP-51 projections (bookmarks /
                // search-relays) created_at guard.
                Some(existing) if event.created_at < existing.created_at => false,
                Some(existing) if existing.members == members => {
                    // Same members at an equal-or-newer timestamp: bump the
                    // stored `created_at` so a later older event is still
                    // rejected, but emit NO change notification (members are
                    // identical, matching the prior dedup-by-members behavior).
                    store.lists.insert(
                        list_id,
                        ListEntry {
                            members,
                            created_at: event.created_at,
                        },
                    );
                    false
                }
                _ => {
                    store.lists.insert(
                        list_id,
                        ListEntry {
                            members,
                            created_at: event.created_at,
                        },
                    );
                    true
                }
            }
        };
        if changed {
            self.notify_changed();
        }
    }
}

#[cfg(test)]
#[path = "people_list_tests.rs"]
mod tests;
