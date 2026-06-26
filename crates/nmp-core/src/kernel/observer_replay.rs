//! ADR-0062 — observer-scoped read-model catch-up.
//!
//! When a late-joining per-open feed (Chirp author/thread profile) registers a
//! `ObservedProjectionSink` AFTER events matching its interest have already been
//! accepted and cached in the in-memory read-cache (`Kernel::events`), it
//! misses those events because the global `notify_observers` fan-out is
//! one-shot and per-observer activation replay is not part of the live ingest
//! path.
//!
//! This module provides the `open_interest_with_observer_replay` kernel method
//! that wires together:
//!
//! 1. `register_interest` — the normal interest front-door (`EnsureAbsent`),
//!    which triggers the relay-subscribe path. Unchanged and DO NOT TOUCH.
//! 2. `replay_read_cache_to_observer` — scan the in-memory `events` read-cache
//!    for matching events and deliver them to the specific muted observer via
//!    `notify_event_observer_by_id`. This deliberately targets ONLY the one
//!    observer, not the global fan-out, so already-active observers do NOT
//!    receive duplicate deliveries.
//! 3. `activate_observer_scoped` — promote the muted observer to scoped live
//!    delivery so subsequent `notify_observers` calls reach it only for events
//!    matching the declared observed interest.
//!
//! ## Dedup invariant
//!
//! The replay step reads from `self.events` (the in-memory read-cache) which is
//! populated by `project_accepted_event` ONLY for events that passed through the
//! live ingest chokepoint (ADR-0057). Events served from the store by the
//! cache-serve path (continuation.rs) are deduplicated against this same cache
//! (`events_cache.contains_key` — line 99, DO NOT TOUCH). So replaying from
//! `self.events` means the observer sees exactly the events that already fired
//! the global fan-out — no store query, no double-count risk.
//!
//! ## Store point-lookup extension (NIT-1)
//!
//! For shapes carrying an `event_ids` set (e.g. the `{ids:[root]}` thread-root
//! hydration shape), `replay_read_cache_to_observer` additionally looks up each
//! id that is NOT in `self.events` via `EventStore::get_by_id` (a O(1)
//! point-read).  The thread root is NOT pinned by `open_view_pins` (the open
//! interest is keyed on `#e` replies, not on the root id itself), so it is a
//! prime LRU-eviction candidate in a long session (>1 000 cached events).
//! Without this extension the root would be absent from the catch-up batch
//! until a live relay re-fetch completed.  The store fetch is READ-ONLY and
//! does NOT mutate `self.events`, metrics, or served-event state — only the
//! observer receives the event, through the same `notify_event_observer_by_id`
//! path as the RAM-tier hits.
//!
//! ## D9 clock clamp
//!
//! `replay_read_cache_to_observer` clamps `created_at` to `now_secs()` for
//! future-dated events, mirroring `ingest/projection.rs:87`. The authoritative
//! store retains the original timestamp; the observer receives only the
//! display-safe clamped value.
//!
//! ## Ordering
//!
//! Events are selected newest `limit` (sorted by `(created_at, id)`), then
//! delivered oldest-first (the tail of the sorted slice), mirroring
//! `continuation.rs:124` — so the feed observer applies events in the same
//! chronological order as the live and cache-serve ingest paths.

use super::Kernel;
use crate::actor::ObservedProjectionId;
use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
use crate::planner::{InterestShape, LogicalInterest};
use crate::subs::SubIdentity;
use crate::substrate::KernelEvent;

/// Replay request carrying the registration parameters for targeted
/// read-cache catch-up (ADR-0062 §6).
pub(crate) struct ObserverReplayRequest {
    /// The muted observer id to deliver replayed events to.
    pub observer_id: ObservedProjectionId,
    /// Filter shapes used to match events in the read-cache.
    /// A single `OpenObservedInterest` command may carry multiple shapes
    /// (e.g. thread feed: `#e` replies shape + root-by-id shape) — all
    /// matching events across all shapes are delivered (union).
    pub shapes: Vec<InterestShape>,
    /// Maximum number of events to replay (newest-first selection,
    /// oldest-first delivery). Use the feed's visible window limit.
    pub limit: usize,
}

impl Kernel {
    /// ADR-0062 — open an interest AND immediately replay matching in-memory
    /// cached events to the nominated muted observer, then activate it.
    ///
    /// Calls `register_interest` with `EnsureAbsent` (unchanged relay-subscribe
    /// path), then UNCONDITIONALLY replays `self.events` to the observer
    /// (regardless of whether `changed` is true — the slot may already exist
    /// from a previous multi-owner open, but the new observer still needs its
    /// catch-up). Then promotes the observer from muted to active.
    ///
    /// Returns `true` iff the interest was newly installed.
    pub(crate) fn open_interest_with_observer_replay(
        &mut self,
        identity: SubIdentity,
        interest: LogicalInterest,
        replay: ObserverReplayRequest,
        reason: &'static str,
    ) -> bool {
        let live_shape = interest.shape.clone();

        // Step 1: normal interest registration (UNCHANGED — do not gate replay
        // on the `changed` outcome; a multi-owner slot returns changed:false
        // for a second subscriber but the new observer still needs its replay).
        let outcomes = self.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::EnsureAbsent,
            }],
            reason,
        );

        // Step 2: targeted read-cache replay. Always runs — the observer is
        // muted and has received nothing yet regardless of the slot state.
        let replayed = self.replay_read_cache_to_observer(&replay);
        let _ = replayed; // count is informational; callers don't need it

        // Step 3: promote muted → scoped-live so future fan-out includes it
        // only for events matching the observed interest.
        if let Some(slot) = &self.event_observers {
            crate::actor::activate_observer_scoped(slot, replay.observer_id, live_shape);
        }

        outcomes[0].newly_installed
    }

    /// Scan `self.events` (the in-memory read-cache) for entries matching any
    /// of `replay.shapes`, then — for shapes carrying a non-empty `event_ids`
    /// set — additionally look up each id absent from the RAM cache via the
    /// store's `peek_by_id` point-read (NIT-1 fix: serves evicted thread roots).
    /// Selects the newest `replay.limit` events across both sources and delivers
    /// them oldest-first to `replay.observer_id` via `notify_event_observer_by_id`.
    ///
    /// # Invariants
    ///
    /// - MUST NOT call `feed_served_event` / `project_accepted_event` /
    ///   `note_store_mutation` / `cache_event_for_matching_open_interest`.
    /// - MUST NOT mutate `self.events`, metrics, served-interest state,
    ///   pending serves, wakeups, or `changed_since_emit`.
    /// - Store lookup is READ-ONLY: `peek_by_id` (non-stamping point-read,
    ///   no LRU touch, no write txn) only, no scan.
    /// - `created_at` is D9-clamped to `now_secs()` (future-dated defence,
    ///   same as `ingest/projection.rs:87`).
    /// - Dedup: a RAM-matched id is never fetched from the store; an id
    ///   referenced by multiple shapes is fetched at most once.
    ///
    /// Returns the number of events delivered.
    fn replay_read_cache_to_observer(&self, replay: &ObserverReplayRequest) -> usize {
        if replay.shapes.is_empty() || replay.limit == 0 {
            return 0;
        }

        let now = self.now_secs();

        // Collect all matching entries from the read-cache (owned copies so
        // we don't hold a borrow across the mutable `notify_event_observer_by_id`).
        struct CachedEntry {
            created_at: u64,
            id: String,
            author: String,
            kind: u32,
            tags: Vec<Vec<String>>,
            content: String,
        }

        let needs_relay_provenance = replay.shapes.iter().any(|shape| shape.relay_pin.is_some());

        let mut matched: Vec<CachedEntry> = Vec::new();
        for (id, stored) in &self.events {
            let relay_provenance = if needs_relay_provenance {
                super::provenance::relay_urls_for_event(&*self.store, id)
            } else {
                Vec::new()
            };
            let matches = replay.shapes.iter().any(|shape| {
                crate::substrate::observed_shape_matches_fields(
                    shape,
                    id,
                    &stored.author,
                    stored.kind,
                    stored.created_at,
                    &stored.tags,
                    &relay_provenance,
                )
            });
            if matches {
                matched.push(CachedEntry {
                    created_at: stored.created_at,
                    id: id.clone(),
                    author: stored.author.clone(),
                    kind: stored.kind,
                    tags: stored.tags.clone(),
                    content: stored.content.clone(),
                });
            }
        }

        // ── Store point-lookups for explicit event_ids evicted from RAM ──────
        //
        // For every shape carrying a non-empty `event_ids` set, look up each
        // id absent from the in-RAM cache via the store's O(1) point-read.
        // The thread-root id is the canonical case: the open interest is keyed
        // on `#e` replies (keeping the root un-pinned by `open_view_pins`), so
        // in a long session the root is a prime LRU-eviction candidate.
        //
        // Constraints — READ-ONLY:
        //   • No `self.events` mutation — the cache is never written here.
        //   • No metrics / served-event accounting / wakeups.
        //   • Only `event_ids` consulted — no store scan for other dimensions.
        //   • Dedup across shapes: collect unique candidates first, then fetch.
        //
        // The collected candidates exclude ids already present in `self.events`
        // (those were handled by the RAM scan above) so there is no risk of
        // double-delivering a root that IS still in RAM.
        {
            // Unique hex ids absent from the RAM cache, across all shapes.
            // BTreeSet gives deterministic iteration order and free dedup.
            let mut store_candidates: std::collections::BTreeSet<&str> =
                std::collections::BTreeSet::new();
            for shape in &replay.shapes {
                for hex_id in &shape.event_ids {
                    if !self.events.contains_key(hex_id.as_str()) {
                        store_candidates.insert(hex_id.as_str());
                    }
                }
            }
            // Point-lookup each candidate. `hex_to_pubkey_bytes` converts the
            // 64-char hex id to the store's `[u8; 32]` key; skip malformed
            // entries (impossible in practice — relay-sourced ids are always
            // valid 64-char hex after the ingest gate).
            for hex_id in store_candidates {
                let Some(id_bytes) = super::hex_to_pubkey_bytes(hex_id) else {
                    continue;
                };
                // BLOCK-2: use peek_by_id (pure read) — must not stamp the LRU
                // access counter or open a write transaction.
                let Ok(Some(stored)) = self.store.peek_by_id(&id_bytes) else {
                    continue;
                };
                let raw = &stored.raw;
                // BLOCK-1: re-check the shape predicate against the fetched event.
                // The `event_ids` set only gates the point-lookup; the full shape
                // (kinds, authors, since, until, tags) must also match.
                let relay_provenance = if needs_relay_provenance {
                    super::provenance::relay_urls_for_event(&*self.store, &raw.id)
                } else {
                    Vec::new()
                };
                let shape_match = replay.shapes.iter().any(|shape| {
                    crate::substrate::observed_shape_matches_fields(
                        shape,
                        &raw.id,
                        &raw.pubkey,
                        raw.kind,
                        raw.created_at,
                        &raw.tags,
                        &relay_provenance,
                    )
                });
                if !shape_match {
                    continue;
                }
                matched.push(CachedEntry {
                    created_at: raw.created_at,
                    id: raw.id.clone(),
                    author: raw.pubkey.clone(),
                    kind: raw.kind,
                    tags: raw.tags.clone(),
                    content: raw.content.clone(),
                });
            }
        }

        if matched.is_empty() {
            return 0;
        }

        // Select newest `limit` events: sort ascending by (created_at, id),
        // then keep the tail (newest-first conceptually, but we deliver
        // oldest-first so we use the tail of the ascending sort).
        matched.sort_unstable_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let start = matched.len().saturating_sub(replay.limit);

        // Deliver oldest-first (the window is already in ascending
        // created_at order — mirrors continuation.rs:124 where collected
        // is reversed to feed oldest-first).
        let mut delivered = 0;
        for entry in &matched[start..] {
            let event = KernelEvent {
                id: entry.id.clone(),
                author: entry.author.clone(),
                kind: entry.kind,
                // D9: clamp future-dated events to now (mirrors
                // ingest/projection.rs:87).
                created_at: entry.created_at.min(now),
                tags: entry.tags.clone(),
                content: entry.content.clone(),
                // relay_provenance filled by notify_event_observer_by_id via
                // the store lookup (same path as notify_event_observers).
                relay_provenance: Vec::new(),
            };
            if self.notify_event_observer_by_id(replay.observer_id, &event) {
                delivered += 1;
            }
        }
        delivered
    }
}
