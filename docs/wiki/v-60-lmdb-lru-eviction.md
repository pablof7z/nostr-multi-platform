---
title: V-60 — LMDB LRU Eviction (MEDIUM · Section 1)
slug: v-60-lmdb-lru-eviction
summary: "V-60 LMDB LRU eviction (MEDIUM): LRU access tracking, gc_step clock injection, index-drift safety, PR #841."
tags:
  - backlog
  - V-60
  - LMDB
  - LRU
  - eviction
  - gc
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-60 — LMDB LRU Eviction (MEDIUM · Section 1)

> V-60 LMDB LRU eviction (MEDIUM): LRU access tracking, gc_step clock injection, index-drift safety, PR #841.

## Overview

V-60 is a MEDIUM-priority Section-1 backlog item: implement LRU eviction for the event store. V-60 was unblocked by V-59's clock injection (merged earlier in the wave). The implementation extended GcBudget with a ceiling, threaded the kernel clock into gc_step (the deferred D7 fix), added LRU access tracking to both backends (mem HashMap, LMDB sub-db + AtomicU64 seq), and eviction skips pinned/claimed events while avoiding tombstoning — evicted events stay re-fetchable from relays. [^4edd4-157]


Eviction cleans every secondary index — including the relay_index introduced in V-52 — when removing events, preventing index drift of the same class as the claim_sub_index panic. [^4edd4-237]
## Review — Two High-Risk Areas Scrutinized

The review specifically scrutinized: (1) index drift on eviction — must clean V-52's relay_index plus all secondary indexes, or it's the same class as the claim_sub_index panic; (2) the write-on-read trade-off. The review APPROVED contingent on CI with all 5 high-risk areas passing: eviction cleans every secondary index (Test 5 asserts evicted ids vanish from list_events_seen_on), pinned events provably survive (Test 4), the write-on-read is safe and bounded to point-reads, D7 clock injected, both-backend parity. Two non-blocking follow-ups: a re-insert-after-evict test (behavioral no-tombstone proof) and an O(N)-per-GC-step observability note. [^4edd4-158]

## Merge

V-60 landed as PR #841 (master at bb8bc105), completing the correctly-prioritized wave's unblocked Section-1 items. [^4edd4-159]

## See Also

