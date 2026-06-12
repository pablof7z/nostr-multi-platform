//! Chunked-continuation drain for one `PendingCacheServe`.
//!
//! `serve_chunk` processes one store query at a time under the shared per-tick
//! budget, advancing the resume cursor between ticks so a long serves does not
//! stall the actor thread.
//!
//! `Etag`/`Ptag` queries do not carry `until` cursors (the index does not
//! support time-bounded pagination). When their scan returns fewer events than
//! the visit limit the chunk advances to the next query; if it returns a full
//! visit-limit load the chunk treats this as if the scan terminated (no cursor
//! to lower) and also advances. This is a conservative over-serve: for large
//! stores a Ptag/Etag scan may miss the deep tail on the first chunk but
//! relay delivery fills the gap (the mechanism is "store first, relay
//! refinement second" — not "store only").

use super::queries::{query_since_mut, query_until, query_until_mut};
use super::PendingCacheServe;
use super::super::Kernel;
use super::super::types::StoredEvent;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::KernelEvent;

/// One store-served event collected during the immutable-borrow phase of
/// `serve_chunk`. Extended with `sig` (the Schnorr signature) so that
/// kind:1059 events (and any other raw-observer targets) can be re-serialized
/// as verbatim NIP-01 JSON by `feed_served_event` when firing
/// `notify_raw_event_observers`.
pub(super) struct CollectedEvent {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) kind: u32,
    pub(super) created_at: u64,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    /// Schnorr signature (lowercase hex, 128 chars). Preserved so the raw
    /// observer path can reconstruct the verbatim signed event without
    /// re-verification. The signature was verified at the original ingest gate
    /// (`VerifiedEvent::try_from_raw`) — replaying it here does not expand the
    /// trust boundary.
    pub(super) sig: String,
    /// Whether this served event should be dispatched through
    /// `notify_raw_event_observers` in addition to `notify_event_observers`.
    /// Set at collection time from the `PendingCacheServe::needs_raw_dispatch`
    /// flag (which was derived at enqueue time from `shape_needs_raw_observer_dispatch`).
    pub(super) needs_raw_dispatch: bool,
}

impl Kernel {
    /// Drain as much of one pending serve as `tick_remaining` allows.
    ///
    /// Returns `true` when the serve is finished (all queries exhausted or
    /// depth satisfied) — the caller then records the completion key.
    pub(super) fn serve_chunk(
        &mut self,
        pending: &mut PendingCacheServe,
        tick_remaining: &mut usize,
        total_served: &mut usize,
    ) -> bool {
        while pending.query_idx < pending.queries.len() {
            if pending.remaining_depth == 0 {
                return true;
            }
            if *tick_remaining == 0 {
                return false;
            }

            // Aggregate-window floor: once the timeline already holds a full
            // visible window, a timeline-bound query only needs events that
            // would beat the window edge. Computed fresh per chunk — the
            // floor rises as the drain progresses, collapsing late authors'
            // scans to near-zero work. `since` is inclusive so window-edge
            // ties are kept (over-serve is safe; under-serve is not).
            let floor = if pending.timeline_bound && self.timeline.len() >= self.visible_limit {
                self.timeline
                    .get(self.visible_limit - 1)
                    .and_then(|id| self.events.get(id))
                    .map(|e| e.created_at)
            } else {
                None
            };

            let query = &pending.queries[pending.query_idx];
            let mut effective = query.clone();
            if let Some(floor_ts) = floor {
                if let Some(since) = query_since_mut(&mut effective) {
                    *since = Some(since.map_or(floor_ts, |s| s.max(floor_ts)));
                }
            }

            let visit_limit = (*tick_remaining).min(pending.remaining_depth.max(1));
            let prev_until = query_until(query);

            // Phase 1 — collect (immutable borrow of the events cache).
            let mut collected: Vec<CollectedEvent> = Vec::new();
            let mut visited = 0usize;
            let mut last_visited_created_at: Option<u64> = None;
            {
                let store = std::sync::Arc::clone(&self.store);
                let events_cache = &self.events;
                let serve_target = pending.remaining_depth;
                let needs_raw = pending.needs_raw_dispatch;
                let _ = store.query_visit(&effective, visit_limit, &mut |ev| {
                    visited += 1;
                    last_visited_created_at = Some(ev.raw.created_at);
                    // Live→serve dedup: already reflected in projections.
                    if !events_cache.contains_key(&ev.raw.id) {
                        collected.push(CollectedEvent {
                            id: ev.raw.id.clone(),
                            author: ev.raw.pubkey.clone(),
                            kind: ev.raw.kind,
                            created_at: ev.raw.created_at,
                            tags: ev.raw.tags.clone(),
                            content: ev.raw.content.clone(),
                            sig: ev.raw.sig.clone(),
                            needs_raw_dispatch: needs_raw,
                        });
                        if collected.len() >= serve_target {
                            return std::ops::ControlFlow::Break(());
                        }
                    }
                    std::ops::ControlFlow::Continue(())
                });
            }

            // Budget accounting: visits are the actor work (index walk +
            // filter), so they consume the tick budget even when deduped.
            *tick_remaining = tick_remaining.saturating_sub(visited.max(1).min(*tick_remaining));

            // Phase 2 — feed oldest-first so each insert lands near the tail
            // of the sorted timeline deque (cheaper on average).
            let served = collected.len();
            collected.reverse();
            for ev in collected {
                self.feed_served_event(ev);
            }
            pending.remaining_depth = pending.remaining_depth.saturating_sub(served);
            *total_served += served;

            let exhausted = visited < visit_limit;
            if exhausted {
                // Index has no more matches below the cursor — next query.
                pending.query_idx += 1;
                continue;
            }

            // Etag/Ptag: no cursor to lower; advance to next query to avoid
            // re-scanning the same head on the next chunk. For deep stores
            // this may miss the tail — relay delivery fills the gap (ADR §9
            // "store first, relay refinement second").
            if query_until_mut(&mut pending.queries[pending.query_idx]).is_none() {
                pending.query_idx += 1;
                continue;
            }

            // More events may remain: lower the resume cursor. `until` is
            // inclusive, so boundary-timestamp events are re-visited next
            // chunk and deduped via the events cache.
            let new_until = last_visited_created_at;
            if served == 0 && new_until == prev_until {
                // Pathological tie: a whole chunk of already-served events at
                // one timestamp and the cursor cannot descend. Advance to the
                // next query rather than livelock; any same-timestamp events
                // beyond the visit limit arrive via the relay path instead.
                pending.query_idx += 1;
                continue;
            }
            match (new_until, query_until_mut(&mut pending.queries[pending.query_idx])) {
                (Some(ts), Some(until)) => *until = Some(ts),
                _ => {
                    // Cursor-less query variant (cannot occur for E1 shapes;
                    // D6: degrade instead of panic) — no resume possible, so
                    // advance rather than re-scan the same head next chunk.
                    pending.query_idx += 1;
                    continue;
                }
            }
            // Stay on this query; the outer loop re-checks budget/depth.
        }
        true
    }

    /// Feed one store-served event into the projection-dispatch path — the
    /// same seam relay-delivered events use after `Inserted | Replaced`
    /// (ADR-0045 §2, step 3).
    ///
    /// For shapes that need raw observer dispatch (kind:1059 DM gift-wraps),
    /// fires BOTH:
    /// - `notify_raw_event_observers` — for Marmot and any other raw-tap
    ///   consumers that still ride the raw observer until PR-2 of the rawtap
    ///   retirement ladder. KEPT until PR-2 removes the raw tap entirely.
    /// - `ingest_dispatcher.dispatch()` — for `IngestParser` consumers (e.g.
    ///   the NIP-17 `DmInboxProjection` migrated to the parser seam in PR-1).
    ///   Dual fan-out is intentional during the PR-1/PR-2 transition window.
    pub(super) fn feed_served_event(&mut self, ev: CollectedEvent) {
        let cached = StoredEvent {
            id: ev.id.clone(),
            author: ev.author.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            tags: ev.tags.clone(),
            content: ev.content.clone(),
            // De-facto `Provenance::LocalStore` marker (see module docs):
            // no relay has confirmed this event in the current session.
            relay_count: 0,
        };

        // Incremental diagnostic counters — mirrors ingest_timeline_event.
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if ev.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(ev.id.clone(), cached);
        self.cached_estimated_store_bytes.set(None);

        let kernel_event = KernelEvent {
            id: ev.id.clone(),
            author: ev.author.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            tags: ev.tags.clone(),
            content: ev.content.clone(),
        };
        self.notify_event_observers(&kernel_event);

        // E2 + PR-1 dual fan-out: for kinds that need raw observer dispatch
        // (kind:1059 DM gift-wraps), fire BOTH the raw-observer tap AND the
        // ingest-dispatcher. During the PR-1/PR-2 transition window:
        //   • `notify_raw_event_observers` — Marmot + any other raw-tap
        //     consumer still riding the raw observer (removed in PR-2).
        //   • `ingest_dispatcher.dispatch()` — NIP-17 `DmInboxProjection`
        //     (migrated to IngestParser in PR-1) and future IngestParser
        //     consumers. The `VerifiedEvent` is reconstructed from the
        //     already-verified raw fields (trust boundary: the store only holds
        //     events that passed `try_from_raw`; re-verification would be
        //     prohibitively expensive on cache-serve).
        //
        // PR-2 removes the `notify_raw_event_observers` call in this block
        // once Marmot is migrated to IngestParser. At that point the `raw`
        // construction and `needs_raw_dispatch` flag also become redundant.
        if ev.needs_raw_dispatch {
            let raw = RawEvent {
                id: ev.id.clone(),
                pubkey: ev.author.clone(),
                created_at: ev.created_at,
                kind: ev.kind,
                tags: ev.tags.clone(),
                content: ev.content.clone(),
                sig: ev.sig.clone(),
            };

            // Fan 1 — raw-observer tap (Marmot + transition consumers).
            // `notify_raw_event_observers` is a fast no-op when no registration
            // exists for this kind (the `raw_event_observers_idle_for_kind`
            // guard is preserved here — PR-2 removes the entire block).
            // Empty string source relay — local-store provenance, no relay URL.
            if !self.raw_event_observers_idle_for_kind(ev.kind) {
                self.notify_raw_event_observers(&raw, "");
            }

            // Fan 2 — ingest-dispatcher (`IngestParser` seam). Reconstruct a
            // `VerifiedEvent` from the already-verified raw fields. This skips
            // re-verification: store events passed `try_from_raw` at original
            // ingest; re-running Schnorr verify on every cache-serve step would
            // be O(events × sessions) overhead. `from_store_verified_unchecked`
            // documents this trust boundary explicitly.
            let verified = VerifiedEvent::from_store_verified_unchecked(raw);
            if let Ok(d) = self.ingest_dispatcher.read() {
                d.dispatch(&verified);
            }
        }

        // Append to the timeline only when the author is in the follow set —
        // mirrors the post-insert branch of `ingest_timeline_event`.
        if self.timeline_authors.contains(&ev.author) {
            self.insert_timeline_id_sorted(ev.id);
        }
    }
}
