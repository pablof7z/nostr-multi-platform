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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# RAM Eviction

## Open-View Pinning

Open-view RAM eviction pins events matching any of four sets per open thread view: the focused event id, the derived root id, referenced_event_ids of the focused event, and the four hydration bookkeeping sets (pending_ids, requested_ids, pending_reply_targets, requested_reply_targets), plus every cached event matching the thread_items() membership predicate. Open-view pins are derived once per GC pass from live view state before any eviction, via Kernel::open_view_pins(). After legacy {1,6} deletion, the open_view_pins re-derivation uses lifecycle.registry().iter_active() + shape.matches_event_with_id(), the same predicate that ingest's matches_active_open_interest uses. The open_view_pins predicate is a copy of the thread_items() predicate rather than a shared function, creating a drift risk if thread_items membership is broadened in the future; flagged as a follow-up to extract a shared membership predicate or fold into #957.

<!-- citations: [^da6b1-37] [^da6b1-60] -->
## Budget & Gate Status

The GC resumable Phase-1 cursor, O(1) Phase-2 count, and hourly tombstone gate constitute honest budgets; the LRU event-count ceiling (HOT_EVENT_CEILING) is disabled until store-claims are wired. LRU recency stamps are deferred to a batch flush (once per 60s GC pass via AtomicU64) instead of a per-read write transaction. Issue #1090 is resolved with floor-coherent eviction: derive_store_pin_set pins every stored event matching an active floored shape with created_at <= shape_floor, reusing the same content-derived floor on the GC path and closing the middle-event eviction hole, and re-enabling the HOT_EVENT_CEILING (10,000) in production. The persisted WatermarkRow machinery (write_watermark/coverage) has zero production writers; only nmp-testing writes rows, so the live since-floor is derived from store content (newest matching event) rather than from persisted watermarks. The dead persisted-watermark machinery is deleted entirely. `GcBudget::production()` now sets `max_total_events = HOT_EVENT_CEILING` (10,000), with `default()` keeping `usize::MAX` for tests. Evicted-root attribution must be reclaimed from the state when `insert_returning_evicted` displaces a root, preventing attribution leaks. The memory one-change fix is threading a derived pin-set into gc_step and deleting the persisted claims sub-db, ClaimerId, and OverPinned, collapsing three pin representations into one.

<!-- citations: [^da6b1-38] [^2e544-32] [^02745-18] [^02745-48] [^2e544-67] [^02745-91] [^02745-106] -->
