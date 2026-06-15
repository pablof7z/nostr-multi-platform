---
title: RAM Eviction
slug: ram-eviction
topic: ram-eviction
summary: "Open-view RAM eviction pins events matching any of four sets per open thread view: the focused event id, the derived root id, referenced_event_ids of the focuse"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019eca83-d126-7fc3-9bd7-e83f65c0a643
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# RAM Eviction

## Open-View Pinning

Open-view RAM eviction pins events matching any of four sets per open thread view: the focused event id, the derived root id, referenced_event_ids of the focused event, and the four hydration bookkeeping sets (pending_ids, requested_ids, pending_reply_targets, requested_reply_targets), plus every cached event matching the thread_items() membership predicate. Open-view pins are derived once per GC pass from live view state before any eviction, via Kernel::open_view_pins(). After legacy {1,6} deletion, the open_view_pins re-derivation uses lifecycle.registry().iter_active() + shape.matches_event_with_id(), the same predicate that ingest's matches_active_open_interest uses. The open_view_pins predicate is a copy of the thread_items() predicate rather than a shared function, creating a drift risk if thread_items membership is broadened in the future; flagged as a follow-up to extract a shared membership predicate or fold into #957. Locally-published events are pinned via a publish-in-flight pin source in derive_store_pin_set while in the publish queue until first relay confirmation or terminal settlement, preventing LRU eviction before the relay echo arrives.

<!-- citations: [^da6b1-37] [^da6b1-60] [^78b50-234] -->
## Budget & Gate Status

GC is wired in production: `actor/mod.rs:2332-2335` fires `kernel.run_gc_step()` at most once per 60s on the actor idle tick. In production, GC is called on a 60-second interval with a budget of 2,000 events and 50 ms per pass. Storage is bounded by pin-aware LRU GC (HOT_EVENT_CEILING) and watermark alone; there is no relevance-gated storage bound. The LRU event-count ceiling (HOT_EVENT_CEILING) is re-enabled in production at 10,000 events; unpinned non-followed events are the first class LRU evicts. LRU recency stamps are deferred to a batch flush (once per 60s GC pass via AtomicU64) instead of a per-read write transaction. Issue #1090 is resolved with floor-coherent eviction: derive_store_pin_set pins every stored event matching an active floored shape with created_at <= shape_floor, reusing the same content-derived floor on the GC path and closing the middle-event eviction hole. The persisted WatermarkRow machinery (write_watermark/coverage) has zero production writers; only nmp-testing writes rows, so the live since-floor is derived from store content (newest matching event) rather than from persisted watermarks. The dead persisted-watermark machinery is deleted entirely. `GcBudget::production()` now sets `max_total_events = HOT_EVENT_CEILING` (10,000), with `default()` keeping `usize::MAX` for tests. Pin-aware LRU GC (`HOT_EVENT_CEILING = 10_000`) is the sole storage bound; the relevance gate is not a storage bound and its volume-limiting role is structurally replaced by LRU eviction of unpinned cold events. The truncation→LRU-skip path in derive_store_gc_inputs (ram_eviction.rs:309-316) must be verified under the store-everything regime to confirm it does not regress into unbounded growth; this is a must-verify test item for the PR. Evicted-root attribution must be reclaimed from the state when `insert_returning_evicted` displaces a root, preventing attribution leaks. gc_step accepts a derived pinned-event set from the kernel (`gc_step(budget, now_secs, pinned: &HashSet<EventId>)` or a `PinProvider` callback) instead of using the persisted claims sub-db; the persisted claims sub-db, `ClaimerId`, and `OverPinned` machinery are deleted. There is no write-time size gate in store insert; mem and LMDB inserts reject malformed, ephemeral, expired, or tombstoned/superseded events but do not cap total rows before write, so removing the relevance gate can increase durable write volume immediately. If sustained ingress exceeds the 2,000-events-per-minute GC budget or pin scans are incomplete, the store can grow beyond the 10,000-event ceiling for multiple ticks and keep growing under sustained load. The PR must include either a stress test or explicit retuning of the production GC budget/ceiling if expected relay admission can exceed the current 10,000-event / 2,000-per-minute rates after the relevance write limiter is removed. Issue #1443 tracks research into an unbounded local event cache where the app can use all events ever ingested; the current 10k LRU ceiling may conflate durable footprint (disk) with RAM working set, and #1443 investigates whether LMDB's memory-mapped design allows an unbounded durable store with only the RAM tier bounded.

<!-- citations: [^da6b1-38] [^2e544-32] [^02745-18] [^02745-48] [^2e544-67] [^02745-91] [^02745-106] [^2e544-400] [^2e544-471] [^019ec-26] [^78b50-54] [^78b50-60] [^78b50-109] [^78b50-111] [^78b50-150] [^78b50-159] [^78b50-168] [^78b50-188] [^78b50-235] [^78b50-251] [^78b50-260] -->
