---
title: Garbage Collection
slug: garbage-collection
topic: garbage-collection
summary: gc_step is wired onto the actor idle tick behind a 60-second wall-clock gate using the kernel's injected Clock seam for replay determinism
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Garbage Collection

## Scheduling & Budget

gc_step is wired onto the actor idle tick behind a 60-second wall-clock gate (Instant-based last_gc), deriving now_secs from the kernel's injected Clock seam for replay determinism, with budget defaults max_events_per_step=2000 and max_duration_ms=50.

<!-- citations: [^da6b1-7] [^da6b1-28] [^da6b1-48] [^da6b1-76] [^da6b1-100] -->
## LRU Eviction Ceiling

The LRU event-count ceiling (HOT_EVENT_CEILING) is disabled in production, defaulting to usize::MAX, until EventStore::claim has production callers wiring pin sets. This resulted in an always-empty pin set that silently evicted events in insert order past 10k, including events referenced by live projections. The lru_stamp method converts every get_by_id hit into an LMDB write transaction, including on the snapshot emit path for claimed_events.

<!-- citations: [^da6b1-8] [^da6b1-29] [^da6b1-49] [^da6b1-64] [^da6b1-77] [^da6b1-101] -->
## GC Phases

GC Phase 1 scan was originally unbounded in duration and deserialization cost, performing a full-store iteration with full event deserialization per event, with max_events_per_step bounding only collected expired ids and max_duration_ms existing only in the delete loop. PR #1094 introduces a resumable Phase 1 cursor (gc_phase1_cursor), storing the created_at of the last-scanned event so each pass resumes with Filter::until(cursor) rather than restarting from the top, replacing the O(N) Phase 2 count with LMDB stat (O(1)). A same-created_at cursor livelock edge case in Phase 1 resumable scanning is tracked as V-118 (#1097): if multiple events share the same created_at timestamp as the cursor, they could be skipped on resume, with both a tactical fix (exclusive bound + no-progress detector) and durable fix (expiration index) documented. Phase-3 tombstone purges are gated to run at most once per GC_TOMBSTONE_PURGE_INTERVAL_SECS = 3600s.

<!-- citations: [^da6b1-9] [^da6b1-30] [^da6b1-50] [^da6b1-65] [^da6b1-78] [^da6b1-87] [^da6b1-102] -->
## In-Memory Bounds & Pin Sets

The kernel's in-memory events, profiles, and seed_contacts HashMaps were originally insert-only with no eviction path, meaning long sessions accumulated every unique event/profile ever seen. Kernel RAM eviction now bounds these maps: events (HWM 1000), profiles (HWM 2000), and seed_contacts (HWM 32). Pin sets are derived from lifecycle.registry().iter_active() plus shape.matches_event_with_id() — the same predicate ingest's matches_active_open_interest uses — ensuring pins stay correct even after the deletion of the author/thread view state machine. Events are pinned by focused id, root, referenced_event_ids, all four hydration bookkeeping sets, and events matching the thread_items() predicate. Profiles are pinned by timeline_authors, profile_claims, active_account, and open-view authors. LMDB safety is guaranteed for all three RAM-evicted maps: they are only populated after verify_and_persist/store.insert returns Inserted|Replaced (D4 ordering), so evicting from RAM loses no data. The kernel's local_profile_intents HashMap accumulates one entry per account (≤2 in practice) and is not bounded, but is not considered a real unbounded accumulation risk.

<!-- citations: [^da6b1-10] [^da6b1-31] [^da6b1-51] [^da6b1-66] [^da6b1-79] [^da6b1-103] -->
## Staged-Out Triggers & Follow-Ups

Memory-warning and insert-overflow GC triggers are staged out because MemoryWarningCapability does not exist anywhere in the codebase. StoreHealth diagnostics surface and runtime-configurable GC ceiling are documented follow-ups to the gc_step wiring, not included in the initial implementation. <!-- [^da6b1-52] -->
