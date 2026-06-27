//! #1088 — Bounded RAM-tier eviction for `events`, `profiles`, and the
//! capability-owned contacts cache (`Arc<dyn ContactsLookup>`).
//!
//! ## Problem
//!
//! The kernel `events` map, the profile cache, and the contacts cache are
//! insert-only: a long session
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
//!   snapshot omit the live `refs.event` row).
//! - **Active open interest** (V-112 / ADR-0042): every cached event that
//!   matches the wire-filter shape of any active `LogicalInterest` in the
//!   planner registry (`self.lifecycle.registry().iter_active()` +
//!   `shape.matches_event_with_id(..)` — the exact predicate
//!   `should_store_event`'s `matches_active_open_interest` clause uses for
//!   admission).  Open views are per-app FlatFeeds backed by
//!   `open_interest`; the feed engine fan-out reads `self.events` with no
//!   store fallback, so an open interest's working set (a non-followed
//!   author's notes, an arbitrary thread, a `#t` feed) would otherwise be
//!   eviction candidates while on screen.  Thread-hydration bookkeeping
//!   moved app-side with the legacy view stack and is no longer a kernel
//!   pin source.
//!
//! ### `profiles` pin set (pubkey → Profile)
//! The follow set is **NOT** a pin source (#1497 amendment 6): the profile
//! cache is a plain bounded LRU decoupled from the follow count. An evicted
//! followed-author profile re-serves from the store on demand (kind:0 is
//! store-first), so pinning every followed author forever is overkill. Only
//! genuinely-unbacked live references are pinned:
//! - The pubkey is in `self.profile_claims` (a UI component is claiming it).
//! - The pubkey is the active account's own key (`self.active_account`).
//! - **Open-interest authors**: the author of every pinned open-interest
//!   event. These feed profile refs for open feeds through the in-memory
//!   profile lookup, which has no store fallback.
//!
//! ### contacts-cache pin set (pubkey → follow list)
//! - The pubkey is `self.active_account` (follow/unfollow actions and the
//!   ReducedSource active-follows feed-session compiler both read
//!   `contacts_lookup().follows(active_account)`).
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
//! 1. Derive the open-interest pins from the live planner registry
//!    ([`Kernel::open_view_pins`]) — computed once, BEFORE any eviction, so
//!    the profile pins see the pre-eviction event set.
//! 2. Collect all non-pinned keys (owned copies — no borrow-split conflicts).
//! 3. Sort by `created_at` ascending (oldest first); tiebreak by key for
//!    determinism.
//! 4. Remove entries from the front until the map length falls to the
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
//! | `profiles` | 4 000 (`PROFILES_RAM_HWM`) | Plain bounded LRU, decoupled from the follow count (#1497 amendment 6). Comfortably holds a large follow set plus browsed profiles; over-bound entries evict oldest-first and re-serve from the store on demand. |
//! | contacts cache | 32 | In practice ≤ a handful (active account + a few peers whose kind:3 arrived). |
//!
//! ## Interaction with #1085 / #957
//!
//! #1085 touches the LMDB-tier `run_gc_step` internals; this module adds a
//! *separate* call site (`evict_ram_caches`) that `run_gc_step` calls before
//! the store GC pass.  The two paths are additive and do not touch each
//! other's code paths.  #957 (retire the legacy author/thread view stack)
//! is DONE (V-112 / ADR-0042): `AuthorViewState`/`ThreadViewState` were
//! deleted, and [`Kernel::open_view_pins`] is derived from the planner's
//! active-interest registry — the read path that replaced
//! `thread_items()`/`author_items()`.

use super::Kernel;
use std::collections::HashSet;

/// Process-lifetime counter: how many events were evicted from the kernel's
/// RAM-tier `self.events` cache in `Kernel::evict_events_cache`. Incremented
/// by the evicted count after every RAM eviction pass. Starts at 0; never
/// decremented. Sibling to `PROCESS_STORE_LRU_EVICTED` in `kernel/mod.rs`.
///
/// Test-support only — zero overhead in production builds.
#[cfg(any(test, feature = "test-support"))]
pub static PROCESS_RAM_EVENTS_EVICTED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// Sibling files (kept out of at-baseline `kernel/mod.rs`): #1090 floor helpers +
// tests, and the K3 Stage C (ADR-0056) floor⇄serve unification lock tests.
#[path = "ram_eviction_floor.rs"]
mod floor;
// K3 Stage D3 (ADR-0056 §3.D3) — pin-below-ledger-floor + coverage-guard set;
// also hosts the D3 kernel-layer tests (`gc_coverage_coherent_d3_tests`).
#[path = "ram_eviction_coverage.rs"]
mod coverage;
#[cfg(test)]
#[path = "gc_floor_coherent_tests.rs"]
mod gc_floor_coherent_tests;

/// High-watermark for `self.events`.  2 × `TIMELINE_CACHE_LIMIT` (500).
pub(super) const EVENTS_RAM_HWM: usize = 1_000;

/// High-watermark for the profile cache — a plain bounded LRU decoupled from
/// the follow count (#1497 amendment 6). A few thousand entries comfortably
/// covers a large follow set plus browsed profiles; over-bound entries evict
/// oldest-first and re-serve from the store on demand (kind:0 is store-first).
pub(super) const PROFILES_RAM_HWM: usize = 4_000;

/// High-watermark for the contacts cache.  Small: it keys on unique
/// pubkeys whose kind:3 was ingested — almost always ≤ a handful in
/// production.  32 is generous.
pub(super) const CONTACTS_RAM_HWM: usize = 32;

/// Summary returned by [`Kernel::evict_ram_caches`] for diagnostics.
#[derive(Debug, Clone, Default)]
pub(crate) struct RamEvictionReport {
    /// Number of entries removed from `self.events`.
    pub events_evicted: usize,
    /// Number of entries removed from the profile cache.
    pub profiles_evicted: usize,
    /// Number of entries removed from the contacts cache.
    pub contacts_evicted: usize,
}

/// Pins derived from the live active-interest registry at eviction time.
/// Computed once per [`Kernel::evict_ram_caches`] pass, BEFORE any eviction,
/// so the profile pins are derived from the pre-eviction event set.
#[derive(Default)]
struct OpenViewPins {
    /// Event ids the open-interest feeds currently read from `self.events`
    /// (no store fallback exists on the feed-engine read path).
    event_ids: HashSet<String>,
    /// Authors of the pinned open-interest events — kept for profile refs in
    /// open feeds.
    profile_pubkeys: HashSet<String>,
}

impl Kernel {
    /// Evict stale entries from the three unbounded in-memory caches
    /// (`events`, the profile cache, the contacts cache) — #1088 RAM-tier half of D8.
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

        // Derive the open-interest pins ONCE, before any eviction, so the
        // profile pins see the full pre-eviction event set (a pinned
        // event author's profile pin must not depend on eviction order).
        let view_pins = self.open_view_pins();

        report.events_evicted = self.evict_events_cache(&view_pins);
        report.profiles_evicted = self.evict_profiles_cache(&view_pins);
        report.contacts_evicted = self.evict_contacts_cache();

        // Invalidate the memoised byte-estimate when any map shrank.
        if report.events_evicted + report.profiles_evicted + report.contacts_evicted > 0 {
            self.cached_estimated_store_bytes.set(None);
        }

        // Test-support: accumulate the per-process RAM-eviction counter.
        #[cfg(any(test, feature = "test-support"))]
        if report.events_evicted > 0 {
            PROCESS_RAM_EVENTS_EVICTED.fetch_add(
                report.events_evicted as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }

        report
    }

    // ─── open-interest pin derivation ──────────────────────────────────────

    /// Compute the open-interest working set from the planner's active
    /// `LogicalInterest` registry (V-112 / ADR-0042 — the legacy
    /// `AuthorViewState`/`ThreadViewState` pin sources were deleted; open
    /// views are now per-app FlatFeeds backed by generic `open_interest`).
    ///
    /// Mirrors the admission path exactly: an event is pinned iff it matches
    /// the wire-filter shape of any active interest — the same
    /// `shape.matches_event_with_id(..)` predicate `should_store_event`'s
    /// `matches_active_open_interest` clause uses in `ingest/timeline.rs`.
    /// Whatever the feed engine would re-admit on arrival must not be
    /// evicted while the interest stays open (the feed-engine read path has
    /// no store fallback).
    ///
    /// Cost: one O(events × active-interests) scan per GC pass; zero-cost
    /// inner predicate when no interest is active.
    fn open_view_pins(&self) -> OpenViewPins {
        let mut pins = OpenViewPins::default();

        let active = self.lifecycle.registry().iter_active();
        if active.is_empty() {
            return pins;
        }

        for event in self.events.values() {
            if active.iter().any(|interest| {
                interest.shape.matches_event_with_id(
                    &event.id,
                    &event.author,
                    event.kind,
                    event.created_at,
                    &event.tags,
                )
            }) {
                pins.event_ids.insert(event.id.clone());
                pins.profile_pubkeys.insert(event.author.clone());
            }
        }

        pins
    }

    // ─── store-tier pin derivation (#1090 Stage 1) ──────────────────────────

    /// Derive the store-tier LRU-eviction pin set for [`Kernel::run_gc_step`].
    ///
    /// Mirrors the RAM-tier `events` pin set ([`Self::evict_events_cache`]) but
    /// targets the store's byte-keyed [`EventStore::gc_step_with_pins`] seam
    /// instead of the kernel's hex-keyed `events` map.  An event is pinned when
    /// any of these hold:
    ///
    /// - its hex id appears in `self.timeline` (the bounded visible feed);
    /// - its hex id is a key in `self.event_claims` (a UI component is holding a
    ///   claim — `event_claims` keys are `kind:pubkey:d_tag` coordinates for
    ///   naddr URIs and hex64 ids for nevent/note URIs; only the hex64 keys map
    ///   to a store `EventId`, coordinate keys are skipped);
    /// - it matches the wire-filter shape of any active open interest
    ///   ([`Self::open_view_pins`] — the same predicate `should_store_event`'s
    ///   `matches_active_open_interest` clause uses for admission);
    /// - **#1090 Stage 2 (floor-coherence):** it is a stored event matching an
    ///   active floored shape with `created_at <= shape_floor`. The live
    ///   `since`-floor for a subscription is content-derived (the `watermark_fn`
    ///   floors each REQ's `since` to the newest stored matching event + 1), so
    ///   LRU eviction of a *middle* event below that floor would punch a
    ///   permanent hole — the floored self-healing REQ only re-requests events
    ///   newer than the floor. Pinning every below-floor stored event closes
    ///   that hole. See [`shape_floor`].
    ///
    /// The result is a set of 32-byte store [`EventId`](crate::store::EventId)s.
    /// Hex strings that do not parse to a 32-byte id (e.g. coordinate keys) are
    /// silently skipped — they have no store row to protect.
    ///
    /// This replaces the deleted persisted-claims sub-db: the pin set is
    /// recomputed from live kernel state on every GC pass and passed straight
    /// into `gc_step_with_pins`, never persisted.
    ///
    /// ## Return value
    ///
    /// Returns `(pins, complete)`. When `complete` is `false` the floor-coherent
    /// scan (#1090 Stage 2) hit its per-tick event-visit budget
    /// ([`floor::PIN_SCAN_MAX_EVENTS`]) before covering all active shapes.
    /// The caller (`run_gc_step`) must then skip LRU eviction for this tick
    /// (pass `max_total_events = usize::MAX`) — we cannot safely evict events
    /// we may not have pinned. The next tick retries from scratch. See #1348.
    pub(crate) fn derive_store_pin_set(&self) -> (HashSet<crate::store::EventId>, bool) {
        let view_pins = self.open_view_pins();

        // Convert a hex64 event-id string into a store `EventId` ([u8; 32]).
        // Returns `None` for any string that is not a valid 32-byte hex id
        // (e.g. a `kind:pubkey:d_tag` coordinate key from `event_claims`).
        fn hex_to_id(hex: &str) -> Option<crate::store::EventId> {
            let parsed = nostr::EventId::from_hex(hex).ok()?;
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(parsed.as_bytes());
            Some(bytes)
        }

        // ADR-0057 — read-your-writes pin source. A locally-published event is
        // routed through the accepted-event chokepoint with `local://publish`
        // provenance and persisted immediately, but it is NOT in `self.timeline`
        // unless the author is followed AND it is a rendered timeline kind (a
        // self-authored kind:1 from a user who does not follow themselves, a
        // kind:7 reaction, a kind:0/3 edit, …). Without a publish-in-flight pin
        // source it could be LRU-evicted before its relay echo arrives — the
        // echo would then re-insert it (no longer a `Duplicate` dedup) and a
        // race could drop the user's own just-published event from the store.
        // Pin every event still in the publish engine's `in_flight` set: an
        // event leaves `in_flight` only on relay confirmation or terminal
        // settlement, which is exactly the window the ADR requires it pinned.
        let in_flight_publish_ids = self
            .publish_engine
            .snapshot()
            .in_flight
            .iter()
            .map(|row| row.event_id.as_str());

        let mut pins: HashSet<crate::store::EventId> = self
            .timeline
            .iter()
            .map(String::as_str)
            .chain(self.event_claims.keys().map(String::as_str))
            .chain(view_pins.event_ids.iter().map(String::as_str))
            .chain(in_flight_publish_ids)
            .filter_map(hex_to_id)
            .collect();

        // #1090 Stage 2 — floor-coherent pins. For each active interest with a
        // content-derived `since`-floor, pin every stored event matching that
        // shape at or below the floor so LRU cannot evict a middle event the
        // floored REQ will never re-request.
        // Returns false when any shape's scan was truncated (D8 budget).
        let complete = self.add_floor_coherent_pins(&mut pins);

        (pins, complete)
    }

    /// Derive the store-tier pin set **and** the matching [`GcBudget`] for one
    /// [`Kernel::run_gc_step`] pass (#1348).
    ///
    /// Wraps [`Self::derive_store_pin_set`] and owns the truncation→budget
    /// decision so `run_gc_step` (in the already-at-baseline `kernel/mod.rs`)
    /// stays a thin call site:
    ///
    /// - production durable LRU disabled (#1480) → empty pins, no store scan;
    /// - finite durable-retention budget → derive pins; if the scan truncates,
    ///   force `max_total_events = usize::MAX` so LRU deletion is skipped for
    ///   this tick. We cannot safely delete events the truncated scan may not
    ///   have pinned; the next tick retries from scratch.
    pub(crate) fn derive_store_gc_inputs(
        &self,
    ) -> (HashSet<crate::store::EventId>, crate::store::GcBudget) {
        // Test-support: use an explicit ceiling when configured via
        // `nmp_app_configure_gc_budget` + `Kernel::set_gc_budget_ceiling`.
        // Production default: `GcBudget::production()` → `max_total_events =
        // usize::MAX` (LRU deletion disabled). The cfg-gated block is
        // dead-code-eliminated in production builds.
        #[cfg(any(test, feature = "test-support"))]
        let base_budget = match self.gc_budget_ceiling {
            Some(ceiling) => crate::store::GcBudget::with_durable_event_ceiling(ceiling),
            None => crate::store::GcBudget::production(),
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let base_budget = crate::store::GcBudget::production();

        if base_budget.max_total_events == usize::MAX {
            return (HashSet::new(), base_budget);
        }

        let (pins, complete) = self.derive_store_pin_set();
        let budget = if complete {
            base_budget
        } else {
            crate::store::GcBudget {
                max_total_events: usize::MAX, // LRU eviction deferred this tick
                ..base_budget
            }
        };
        (pins, budget)
    }

    // `add_floor_coherent_pins` (#1090 Stage 2 + K3 Stage D3 leg 1) lives in the
    // sibling `ram_eviction_coverage.rs` (this file is at the 500-LOC cap).

    // ─── events ────────────────────────────────────────────────────────────

    fn evict_events_cache(&mut self, view_pins: &OpenViewPins) -> usize {
        let len = self.events.len();
        if len <= EVENTS_RAM_HWM {
            return 0;
        }

        // Build the pin set: timeline ids + currently-claimed event ids +
        // the open-interest working set.
        let pinned: HashSet<String> = self
            .timeline
            .iter()
            .cloned()
            .chain(self.event_claims.keys().cloned())
            .chain(view_pins.event_ids.iter().cloned())
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

    fn evict_profiles_cache(&mut self, view_pins: &OpenViewPins) -> usize {
        // ADR-0057 PR 2 — the profile cache is capability-owned
        // (`nmp_nip01::ProfileCache` behind `Arc<dyn ProfileLookup>`); the cache
        // owns the eviction *mechanism* (oldest-first, lexicographic-pubkey
        // tiebreak), the kernel owns the *policy* (which pubkeys are pinned + the
        // HWM). The follow set is NOT pinned (#1497 amendment 6 — plain bounded
        // LRU; an evicted followed-author profile re-serves from the store on
        // demand). Pin only genuinely-unbacked live references: claimed profiles
        // + active account + open-interest authors (the feed-engine read path
        // for open interests has no store fallback).
        let pinned: HashSet<String> = self
            .profile_claims
            .keys()
            .cloned()
            .chain(self.active_account.clone())
            .chain(view_pins.profile_pubkeys.iter().cloned())
            .collect();

        let removed = self.profile_lookup().evict_to(&pinned, PROFILES_RAM_HWM);
        if removed > 0 {
            // ADR-0055 Rung 1 (F4): stamp the removal so profile-derived
            // projections' rev stays coherent (else the host serves an evicted one).
            self.projection_rev_tracker.source_versions.bump_profiles();
        }
        removed
    }

    // ─── contacts (kind:3) ───────────────────────────────────────────────────

    fn evict_contacts_cache(&mut self) -> usize {
        // ADR-0057 PR 3 — the contacts cache is capability-owned
        // (`nmp_nip01::ContactsCache` behind `Arc<dyn ContactsLookup>`); the
        // cache owns the eviction *mechanism* (oldest-`created_at`-first,
        // lexicographic-pubkey tiebreak), the kernel owns the *policy* (the pin
        // set + the HWM). Pin the active account's entry — all safety-critical
        // reads are against this key only; every other entry is a speculative
        // extra (a peer's kind:3 that happened to arrive during the session).
        let pinned: HashSet<String> = self.active_account.clone().into_iter().collect();
        self.contacts_lookup().evict_to(&pinned, CONTACTS_RAM_HWM)
    }
}
