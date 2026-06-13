---
title: Cache-Serve
slug: cache-serve
topic: cache-serve
summary: Cache-serve gates v1 as a v1-blocker; the universal mechanism is a single event-acquisition pipeline where store-serving is the first half and the network REQ i
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
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Cache-Serve

## Universal Cache-Serve

Cache-serve gates v1 as a v1-blocker; the universal mechanism is a single event-acquisition pipeline where store-serving is the first half and the network REQ is the refinement half, running always (cold/warm/offline/online) with no domain-specific staging or offline special-casing. The acceptance test seeds events via the real `handle_event` path (real Schnorr-verify + store), uses real `gift_wrap_with_signer` for the DM, clears in-memory state on cold restart, runs with zero relay connections, and was proven non-vacuous by mutation testing (deleting the Ptag arm → E2 FAIL; deleting Etag → E3 FAIL). Cache-serve depth defaults to 1× the view's visible window; deeper replay is per-interest opt-in only. open_interest_sub, push_interest_and_serve, and ensure_interest_and_serve all route through the shared Kernel::enqueue_interest_cache_serve choke point, with PushInterest and EnsureInterest dispatch arms enqueuing a store-cache-serve alongside interest registration so kind-parsers receive store-resident events on every session. open_uri also serves store-first for resolved targets, ensuring store-first cache-serve for every installed interest. (Previously: PushInterest and EnsureInterest never enqueued cache-serve; both the live and store legs to MarmotIngestParser were dead on relaunch.) The startup interests (is_indexer_discovery REQs) are intentionally excluded from cache-serve because their purpose is a fresh network fetch, not serving cached data. The `since`-floor for REQ subscriptions is derived from store content via `watermark_fn` (which queries the newest matching event), not from persisted `WatermarkRow.synced_up_to` (which has zero production writers, only tests). The cache-serve floored⇒served structural guard asserts that a watermark floor implies a cache-serve delivery, tested non-vacuously against the real production watermark_fn. The real hole is that a middle event evicted below the high-water floor is never re-fetched. Cache-serve uses an aggregate per-tick budget of 2× visible window (160 store-event visits) across all pending serves, with per-query continuation cursors resuming partially-completed interests, rather than a per-interest budget that scales unboundedly with follow count. The budget drain is piggybacked on the gc_step tick. The cache_serve module uses relay_count: 0 as the de-facto Provenance::LocalStore marker pending ADR amendment, rather than a dedicated enum variant. Cache-serve markers are keyed to registry-slot lifetime and cleared on close_interest_sub and RAM eviction, not conflated with store key presence.

<!-- citations: [^da6b1-1] [^da6b1-2] [^78c8e-16] [^2e544-1] [^02745-1] [^78c8e-42] [^da6b1-41] [^78c8e-62] [^da6b1-63] [^78c8e-80] [^da6b1-78] -->
## Post-v1 Follow-ups

The cache-serve §6 guard is currently enumerative (hardcoded 4-shape cases) rather than the ideal structural one-table-read-two-ways form; hardening to structural is a post-v1 follow-up. The four D6 doctrine-lint escape-hatch unwraps in cache_serve should be restructured to if-let pattern matching as a follow-up, since the identical access at queries.rs:156 already uses the cleaner idiom. <!-- [^da6b1-3] -->

## v0.5.0 Scope

The v0.5.0 release includes ADR-0045 universal cache-serve (E1 AuthorKind/KindTime for contact feeds, E2 Ptag/kind:1059 for DM inbox with one decrypt path, E3 Etag/KindDtag/Ptag-mentions for threads/long-form) with the universal offline acceptance test passing with zero relay connections. <!-- [^da6b1-64] -->
