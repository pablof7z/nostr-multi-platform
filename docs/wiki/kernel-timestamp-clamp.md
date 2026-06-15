---
title: Kernel Timestamp Clamp
slug: kernel-timestamp-clamp
topic: event-acquisition
summary: The D9 `created_at` clamp (futureânow) must be applied to the observer-delivered `KernelEvent.created_at` at the chokepoint (hostile-relay defense for all fee
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Kernel Timestamp Clamp

## Timestamp Clamping

The D9 `created_at` clamp (future→now) must be applied to the observer-delivered `KernelEvent.created_at` at the chokepoint (hostile-relay defense for all feed consumers), and the timeline read-cache also applies it; the authoritative store retains the raw wire timestamp for protocol correctness. The D9 clamp must be uniform across ALL observer-notify sites including the cache-serve replay path (`feed_served_event`), not just the live `verify_and_persist` path — a future-dated event served from cache after cold-restart must not warp the feed. Whether an event persists must not depend on mutable runtime state (current follow set, which interests are open at the instant of arrival); the same event stream must produce the same stored state regardless of timing. Repost wrapper timestamps no longer pull a root downward; `ingest_repost` now uses `max(existing_slot_ts, wrapper_ts)`, matching the existing clamp in `ingest_root`. Relay diagnostics must ship raw Unix-ms timestamps over the wire, not pre-formatted relative strings; shells format at render time. Relay diagnostics wall-clock anchoring must be deterministic: anchor `started_unix_ms` once at kernel start and compute `started_unix_ms + event_ms`, eliminating tick-to-tick ms jitter from dual live clock reads.

<!-- citations: [^02745-35] [^02745-58] [^78c8e-103] [^78b50-9] [^78b50-38] [^78b50-108] [^78b50-119] [^78b50-135] [^78b50-158] [^78b50-167] [^78b50-178] [^78b50-187] -->
