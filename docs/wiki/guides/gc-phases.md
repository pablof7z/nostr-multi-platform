---
title: GC Phases and gc_step_with_pins
slug: gc-phases
topic: data-persistence
summary: Production GC runs three phases on a 60-second tick via gc_step_with_pins. Phase 1 expires TTL-expired events, Phase 2 enforces the LRU ceiling, Phase 3 purges tombstone entries (at most once per hour).
tags:
  - capture
volatility: warm
confidence: high
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
---

# GC Phases and gc_step_with_pins

## Overview

GC runs on the actor thread via `run_gc_step` (called from the actor idle tick, same ≤250 ms cadence as cache-serve drain). In production, `gc_step_with_pins` runs three phases in a single pass:

**Phase 1 — Expiry index scan:** Reads the `expiry_index` sub-database (keyed by `expires_at` unix-second, big-endian) and deletes any event whose TTL has passed. Events are only in this index if they carry a `["expiration", "..."]` tag. This is the NIP-40 expiry path.

**Phase 2 — LRU ceiling enforcement:** If the event count exceeds `lru_ceiling` (the configured maximum), evicts the least-recently-accessed events until the store is back within the ceiling. Access order is tracked in the `lru_access` sub-database (keyed by `(last_access_time, event_id)`). Pinned events are excluded from eviction.

**Phase 3 — Tombstone purge (at most once per hour):** Removes entries from `tombstones` and `addr_tombstones` for events that are no longer stored (safe to drop the tombstone once the event it guards against is gone). Rate-limited to once per hour to avoid O(tombstone-set) scans on every GC tick.

## Pinned Events

`gc_step_with_pins` accepts a `&PinnedSet` argument. Pinned events are IDs that the kernel's current read-cache holds alive (events in active timeline projections). The GC Phase 2 eviction path skips any event whose ID is in the pinned set. This prevents live-view events from being evicted mid-render.

The kernel builds the pin set from `events_cache` (the in-RAM event read-cache) before calling `run_gc_step`. This is the `EVICT_RAM_CACHES` interaction: GC-evicting an event that the read-cache no longer pins drops it silently; the projection will re-fetch from the relay on next subscription or store-serve.

## Budgeted vs. Unbudgeted GC

The original `gc_step` (pre-V-117/#1085) ran unbudgeted O(store) scans on the actor thread, causing jank. The current path is budgeted: Phase 1 and Phase 2 process at most `GC_EVICTION_BATCH` events per tick and leave a cursor for the next tick. Phase 3 is bounded by the once-per-hour rate limit. No single GC tick can stall the actor indefinitely.

## Follow Set and capped_contact_follows

The 500-author per-subscription cap was **retired in #1497 (amendment 5/6)**. The follow feed now uses a single `AuthorsKind` multi-author interest with no per-author `limit`. All surfaces that consume the follow set must go through `ContactsLookup` (the capability-owned contacts cache, `Arc<dyn ContactsLookup>`) — never read `follow_set` directly. This ensures the follow-feed interest, timeline membership gate, and GC pinning all see the same set. There is no longer a hardcoded author count cap; the relay wire sends one `REQ` with the full author set.
