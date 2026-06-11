//! #1088 — Bounded RAM-tier eviction for `events`, `profiles`, and
//! `seed_contacts`.
//!
//! ## Problem
//!
//! The three kernel in-memory HashMaps are insert-only: a long session
//! accumulates every unique event/profile ever ingested, violating D8
//! ("working-set bounded").  The LMDB tier was capped in #1069; this module
//! closes the RAM-tier half.
//!
//! ## Safety contract — no eviction of live references
//!
//! Before eviction any entry is checked against the live reference set
//! (the "pin set").  An entry is pinned when:
//!
//! ### `events` pin set (event id → StoredEvent)
//! - The event id appears in `self.timeline` (the bounded visible feed —
//!   sorted, ≤500 entries by `TIMELINE_CACHE_LIMIT`).
//! - The event id is a key in `self.event_claims` (a UI component is
//!   currently holding a claim on it — evicting would make the next
//!   snapshot emit an empty `claimed_events` entry).
//!
//! ### `profiles` pin set (pubkey → Profile)
//! - The pubkey is in `self.timeline_authors` (current follow set — each
//!   timeline item's `author_display_name` / `author_picture_url` are read
//!   from this cache on every snapshot tick via `timeline_item()`).
//! - The pubkey is in `self.profile_claims` (a UI component is claiming it).
//! - The pubkey is the active account's own key (`self.active_account`).
//!
//! ### `seed_contacts` pin set (pubkey → Vec<String> follow list)
//! - The pubkey is `self.active_account` (follow/unfollow actions,
//!   `should_open_timeline`, and `register_follow_feed_for_active_account`
//!   all read `seed_contacts.get(active_account)`).
//!
//! ## LMDB safety
//!
//! All three maps are populated ONLY after `verify_and_persist` (or
//! `store.insert`) returns `Inserted | Replaced` — persisting to LMDB first
//! (D4 single-writer ordering in `ingest/mod.rs`, `ingest/timeline.rs`,
//! `ingest/profile.rs`, `ingest/contacts.rs`).  Evicting from the RAM cache
//! therefore loses no data: the store holds the authoritative copy, and the
//! kernel reloads on demand (via the claim / snapshot / fallback paths).
//!
//! ## Eviction strategy
//!
//! Eviction is **oldest-created_at-first** per map.  On every GC pass:
//! 1. Collect all non-pinned keys (owned copies — no borrow-split conflicts).
//! 2. Sort by `created_at` ascending (oldest first); tiebreak by key for
//!    determinism.
//! 3. Remove entries from the front until the map length falls to the
//!    high-watermark.
//!
//! The O(n) candidate scan runs once per 60-second GC pass — not on every
//! tick or every ingest.
//!
//! ## Bounds chosen
//!
//! | Map | High-watermark | Rationale |
//! |-----|---------------|-----------|
//! | `events` | 2 × `TIMELINE_CACHE_LIMIT` = 1 000 | 500 timeline entries + up to 500 thread/author/oneshot extras. |
//! | `profiles` | 2 × `TIMELINE_AUTHOR_LIMIT` = 2 000 | 1 000 follow-set entries (all pinned) + 1 000 non-followed browsed profiles. |
//! | `seed_contacts` | 32 | In practice ≤ a handful (active account + a few peers whose kind:3 arrived). |
//!
//! ## Interaction with #1085
//!
//! #1085 touches the LMDB-tier `run_gc_step` internals; this module adds a
//! *separate* call site (`evict_ram_caches`) that `run_gc_step` calls after
//! the store GC pass.  The two paths are additive and do not touch each
//! other's code paths.

use super::Kernel;
use std::collections::HashSet;

/// High-watermark for `self.events`.  2 × `TIMELINE_CACHE_LIMIT` (500).
pub(super) const EVENTS_RAM_HWM: usize = 1_000;

/// High-watermark for `self.profiles`.  2 × `TIMELINE_AUTHOR_LIMIT` (1 000).
pub(super) const PROFILES_RAM_HWM: usize = 2_000;

/// High-watermark for `self.seed_contacts`.  Small: this map keys on unique
/// pubkeys whose kind:3 was ingested — almost always ≤ a handful in
/// production.  32 is generous.
pub(super) const SEED_CONTACTS_RAM_HWM: usize = 32;

/// Reserved for future use: per-pass eviction budget if the single-pass
/// approach proves too expensive on very large maps.  Currently unused —
/// eviction removes all excess down to the HWM in one call.
#[allow(dead_code)]
pub(super) const RAM_EVICTION_BATCH: usize = 64;

/// Summary returned by [`Kernel::evict_ram_caches`] for diagnostics.
#[derive(Debug, Clone, Default)]
pub(crate) struct RamEvictionReport {
    /// Number of entries removed from `self.events`.
    pub events_evicted: usize,
    /// Number of entries removed from `self.profiles`.
    pub profiles_evicted: usize,
    /// Number of entries removed from `self.seed_contacts`.
    pub seed_contacts_evicted: usize,
}

impl Kernel {
    /// Evict stale entries from the three unbounded in-memory HashMaps
    /// (`events`, `profiles`, `seed_contacts`) — #1088 RAM-tier half of D8.
    ///
    /// Called from [`Kernel::run_gc_step`] once per GC pass (60-second
    /// wall-clock gate in the actor).  Each call brings each map down to its
    /// high-watermark by removing the oldest non-pinned entries.  The
    /// candidate collection + sort is O(n) in the map size, but runs only
    /// once per 60-second GC pass — not on every tick or every ingest.
    ///
    /// Returns a [`RamEvictionReport`] so the caller can record / surface
    /// the counts.
    pub(crate) fn evict_ram_caches(&mut self) -> RamEvictionReport {
        let mut report = RamEvictionReport::default();

        report.events_evicted = self.evict_events_cache();
        report.profiles_evicted = self.evict_profiles_cache();
        report.seed_contacts_evicted = self.evict_seed_contacts_cache();

        // Invalidate the memoised byte-estimate when any map shrank.
        if report.events_evicted + report.profiles_evicted + report.seed_contacts_evicted > 0 {
            self.cached_estimated_store_bytes.set(None);
        }

        report
    }

    // ─── events ────────────────────────────────────────────────────────────

    fn evict_events_cache(&mut self) -> usize {
        let len = self.events.len();
        if len <= EVENTS_RAM_HWM {
            return 0;
        }

        // Build the pin set: timeline ids + currently-claimed event ids.
        let pinned: HashSet<String> = self
            .timeline
            .iter()
            .cloned()
            .chain(self.event_claims.keys().cloned())
            .collect();

        // Collect eviction candidates as owned Strings to avoid borrow conflicts
        // when we mutably remove them below.  Sort oldest-created_at-first;
        // tiebreak by key for determinism.
        let mut candidates: Vec<(String, u64)> = self
            .events
            .iter()
            .filter_map(|(k, v)| {
                if pinned.contains(k) {
                    None
                } else {
                    Some((k.clone(), v.created_at))
                }
            })
            .collect();
        candidates.sort_unstable_by(|(ka, a), (kb, b)| a.cmp(b).then_with(|| ka.cmp(kb)));

        // Remove oldest entries until we reach the HWM.  Each pass is bounded
        // by `candidates.len()` (non-pinned entries only) so pinned entries are
        // never touched regardless of how many non-pinned entries exist.
        let to_remove = len - EVENTS_RAM_HWM;
        let mut removed = 0usize;
        for (key, _) in candidates.into_iter().take(to_remove) {
            if self.events.remove(&key).is_some() {
                self.metric_stored_events = self.metric_stored_events.saturating_sub(1);
                removed += 1;
            }
        }
        removed
    }

    // ─── profiles ──────────────────────────────────────────────────────────

    fn evict_profiles_cache(&mut self) -> usize {
        let len = self.profiles.len();
        if len <= PROFILES_RAM_HWM {
            return 0;
        }

        // Build the pin set: followed authors + claimed profiles + active account.
        let pinned: HashSet<String> = self
            .timeline_authors
            .iter()
            .cloned()
            .chain(self.profile_claims.keys().cloned())
            .chain(self.active_account.clone())
            .collect();

        // Collect eviction candidates as owned Strings — same borrow-split
        // rationale as `evict_events_cache`.
        let mut candidates: Vec<(String, u64)> = self
            .profiles
            .iter()
            .filter_map(|(k, v)| {
                if pinned.contains(k) {
                    None
                } else {
                    Some((k.clone(), v.created_at))
                }
            })
            .collect();
        candidates.sort_unstable_by(|(ka, a), (kb, b)| a.cmp(b).then_with(|| ka.cmp(kb)));

        let to_remove = len - PROFILES_RAM_HWM;
        let mut removed = 0usize;
        for (key, _) in candidates.into_iter().take(to_remove) {
            if self.profiles.remove(&key).is_some() {
                removed += 1;
            }
        }
        removed
    }

    // ─── seed_contacts ─────────────────────────────────────────────────────

    fn evict_seed_contacts_cache(&mut self) -> usize {
        let len = self.seed_contacts.len();
        if len <= SEED_CONTACTS_RAM_HWM {
            return 0;
        }

        // Pin the active account's entry — all safety-critical reads are
        // against this key only.  All other entries are speculative extras
        // (peers' kind:3 events that happened to arrive during the session).
        let active: Option<String> = self.active_account.clone();

        // Collect as owned Strings to avoid the borrow-split issue.
        let mut candidates: Vec<String> = self
            .seed_contacts
            .keys()
            .filter(|k| Some(k.as_str()) != active.as_deref())
            .cloned()
            .collect();
        // Sort by key for determinism (no created_at stored here).
        candidates.sort_unstable();

        let to_remove = len - SEED_CONTACTS_RAM_HWM;
        let mut removed = 0usize;
        for key in candidates.into_iter().take(to_remove) {
            if self.seed_contacts.remove(&key).is_some() {
                removed += 1;
            }
        }
        removed
    }
}
