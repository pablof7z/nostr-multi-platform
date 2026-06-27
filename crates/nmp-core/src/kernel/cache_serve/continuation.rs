//! Chunked-continuation drain for one `PendingCacheServe`.
//!
//! `serve_chunk` processes one store query at a time under the shared per-tick
//! budget, advancing the resume cursor between ticks so a long serves does not
//! stall the actor thread.
//!
//! Every `StoreQuery` variant — including the generic `Tags` tag scan — carries
//! a `since`/`until` window, so each query is time-bounded and pages by lowering
//! its `until` cursor between chunks. When a scan returns fewer events than the
//! visit limit the chunk advances to the next query; otherwise it lowers the
//! resume cursor and continues on the same query next tick.

use super::super::types::StoredEvent;
use super::super::Kernel;
use super::queries::{query_since_mut, query_until, query_until_mut};
use super::PendingCacheServe;
use crate::store::RawEvent;
use nmp_store::__nmp_core_internal;

/// One store-served event collected during the immutable-borrow phase of
/// `serve_chunk`. Extended with `sig` (the Schnorr signature) so that
/// the `VerifiedEvent` can be reconstructed for the `IngestParser` dispatcher
/// without re-running Schnorr verification.
pub(super) struct CollectedEvent {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) kind: u32,
    pub(super) created_at: u64,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    /// Schnorr signature (lowercase hex, 128 chars). Preserved so the
    /// `project_accepted_event` fan-out can reconstruct the verbatim signed
    /// event without re-verification. The signature was verified at the
    /// original ingest gate (`VerifiedEvent::try_from_raw`) — replaying it
    /// here does not expand the trust boundary.
    pub(super) sig: String,
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
            match (
                new_until,
                query_until_mut(&mut pending.queries[pending.query_idx]),
            ) {
                (Some(ts), Some(until)) => *until = Some(ts),
                _ => {
                    // No descendable cursor timestamp this chunk (e.g. nothing
                    // newly visited; D6: degrade instead of panic) — no resume
                    // possible, so advance rather than re-scan the same head.
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
    /// For shapes where at least one registered `IngestParser` is interested in
    /// the served kind (`needs_ingest_parser_dispatch = true`, set at enqueue
    /// time by querying `EventIngestDispatcher::is_interested`):
    /// - `ingest_dispatcher.dispatch()` — all registered parsers for the kind
    ///   receive the event. This covers NIP-17 `DmInboxProjection` (kind:1059,
    ///   PR-1), Marmot `MarmotIngestParser` (kind:1059, PR-2), and all-kinds
    ///   range parsers (e.g. chirp-tui's `RawCacheIngestParser`, `0..u32::MAX`).
    ///
    /// The old hardcoded `#p`+kind:1059 allowlist is replaced by the
    /// `is_interested` check at enqueue time — any registered parser now
    /// transparently causes dispatch without code changes here.
    ///
    /// Note: the live ingest path (`kernel/ingest/mod.rs`) STILL calls
    /// `notify_raw_event_observers` for live-delivery consumers like the `hl`
    /// app's nostrdb mirror (verbatim-forwarding consumers). Those are LIVE
    /// relay delivery consumers — cache-serve intentionally does NOT fan out to
    /// the raw tap. The `needs_ingest_parser_dispatch` flag and `sig` field are
    /// retained here because they are used to reconstruct the `VerifiedEvent`
    /// for the `IngestParser` dispatcher (gift-wrap decryption needs the `sig`).
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

        // ADR-0057 — the SINGLE shared post-store projection fan-out. The
        // cache-serve replay path runs EXACTLY the same
        // `Kernel::project_accepted_event` the live ingest chokepoint
        // (`ingest_accepted_event`) runs, so the two cannot diverge:
        //   - NIP-parser dispatch (kind:0 profile, kind:10002 mailbox, kind:10050
        //     DM-relay, kind:1059 gift-wraps, …) writes the capability caches,
        //   - the transition sweep bumps `profiles_ver` / fires
        //     `on_mailbox_changed` / `on_dm_relays_changed` so incremental
        //     projections re-emit after a cold-restart cache-serve (without this,
        //     a stored kind:0 served on restart left profile projections
        //     `Unchanged` → stale UI), and
        //   - the app-observer notify is D9-clamped (a future-dated event served
        //     from the store on restart cannot pin to the top of the feed).
        //
        // ADR-0045 invariant: cache-serve does NOT call `store.insert` (the event
        // is already on disk — this path read it from the store); only the
        // POST-store projection fan-out is shared. The `VerifiedEvent` is
        // reconstructed from the already-verified raw fields (trust boundary: the
        // store only holds events that passed `try_from_raw`; re-verification
        // would be prohibitively expensive on cache-serve). No raw-observer
        // fan-out is emitted here — the verbatim-signed-event tap fires only on
        // live relay ingest. The `needs_ingest_parser_dispatch` flag is retained
        // on `CollectedEvent` (set at enqueue from `EventIngestDispatcher::is_interested`)
        // but the projection helper now always runs: its dispatch is a no-op when
        // no parser matches, and the transition sweep + notify must run for EVERY
        // served event regardless of parser interest.
        let raw = RawEvent {
            id: ev.id.clone(),
            pubkey: ev.author.clone(),
            created_at: ev.created_at,
            kind: ev.kind,
            tags: ev.tags.clone(),
            content: ev.content.clone(),
            sig: ev.sig.clone(),
        };
        let verified = __nmp_core_internal::from_store_verified_unchecked(raw);
        self.project_accepted_event(&verified);

        // Append to the timeline only when the author is in the follow set —
        // mirrors the post-insert branch of `ingest_timeline_event`.
        if self.timeline_authors.contains(&ev.author) {
            self.insert_timeline_id_sorted(ev.id);
        }
    }
}
