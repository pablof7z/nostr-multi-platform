---
title: Kernel Timestamp Clamp
slug: kernel-timestamp-clamp
topic: event-acquisition
summary: Kernel fan-out must clamp `created_at` on `KernelEvent` to `now_secs` for the observer-visible timestamp while preserving the wire timestamp in `StoredEvent` fo
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Kernel Timestamp Clamp

## Timestamp Clamping

Kernel fan-out must clamp `created_at` on `KernelEvent` to `now_secs` for the observer-visible timestamp while preserving the wire timestamp in `StoredEvent` for protocol correctness, preventing future-dated events from warping the feed. Repost wrapper timestamps no longer pull a root downward; `ingest_repost` now uses `max(existing_slot_ts, wrapper_ts)`, matching the existing clamp in `ingest_root`. If a D9-style `created_at` clamp ever needs to be kernel-wide rather than feed-ordering-only, the right move is a single `kernel_event_from_stored` constructor that owns the clamp so the rule cannot drift across call sites. Relay diagnostics must ship raw Unix-ms timestamps over the wire, not pre-formatted relative strings; shells format at render time. Relay diagnostics wall-clock anchoring must be deterministic: anchor `started_unix_ms` once at kernel start and compute `started_unix_ms + event_ms`, eliminating tick-to-tick ms jitter from dual live clock reads.

<!-- citations: [^02745-35] [^02745-58] [^78c8e-103] -->
