---
title: Watermark Removal
slug: watermark-removal
topic: event-acquisition
summary: The live since-floor for REQ subscriptions is derived from store content (newest matching event per author/coord/tag) via watermark_fn, not from persisted Water
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
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:c9a794f6-6ad7-4ee9-a620-fc342fd495c3
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Watermark Removal

## Watermark Removal

The live since-floor for REQ subscriptions is derived from store content (newest matching event per author/coord/tag) via watermark_fn, not from persisted WatermarkRow.synced_up_to — which has zero production writers. The persisted watermark machinery (WatermarkKey/WatermarkRow/SyncMethod/Coverage, read_watermark/write_watermark/coverage trait methods, LMDB watermarks sub-db) has been deleted as dead code.

The address-pointer (KindDtag) watermark branch takes max over coords and ignores coords with zero stored events, which is internally inconsistent with the authors branch's min/abort rule (V-118).

One stray stored event per author permanently suppresses that author's backfill because the watermark fn floors since above the author's entire history.

Enabling Phase-2 LRU eviction will punch permanent holes under the watermark floor because evicted events are never re-fetched (the standing sub never REQs below watermark+1), and the ADR-0045 structural guard checks shape-class coverage not data completeness.

NEG-OPEN inherits the watermark-floored since, so set reconciliation cannot repair below-floor gaps — the correctness layer is reduced to a bandwidth optimization of the already-truncated REQ.

The persisted claims sub-db, ClaimerId, and OverPinned machinery are deleted; gc_step receives a derived pin set from event_claims + timeline + active interests.

Plan-input memoization at the `recompile_and_diff` seam (Fix A) must hash the full input tuple including the watermark store generation; omitting it causes stale `since` values and silent under-fetch. <!-- [^c9a79-30] -->

Milestone/coverage/status.rs sites read only read-caches (self.timeline, self.profiles, self.seed_contacts, self.events) or wire-sub since_floor, never self.store event counts; decoupling admission from persistence causes no breakage. <!-- [^78b50-227] -->

<!-- citations: [^2e544-357] [^2e544-359] [^2e544-358] [^02745-109] [^02745-121] [^02745-136] [^2e544-381] [^2e544-403] [^2e544-442] [^2e544-489] -->
## T129 Watermark Rewrite and Rule 1 Reversal

R4 defects 2 (T129 watermark rewrite) and 4 (Rule 1 wildcard absorption) were documented, tested features, not bugs; their reversal was escalated as owner decisions (#1281, #1282) rather than merged autonomously. <!-- [^02745-110] -->

## since=None Backfill Exemption

Issue #1281's since=None backfill semantics is decided: since=None is exempted from the T129 watermark rewrite for non-Tailing (backfill/OneShot) subscriptions, while Tailing subscriptions keep narrowing to watermark+1. For a non-Tailing interest, a None since stays None (unbounded); only an existing Some(t) floor is raised to max(t, watermark+1). Tailing interests always apply T129 watermark narrowing (since=watermark+1).

<!-- citations: [^02745-122] [^02745-137] -->
#1281's since=None backfill semantics is the single genuinely-needs-owner decision — whether to exempt unbounded (since=None) interests from the T129 watermark rewrite. since=None interests are exempted from the T129 watermark rewrite: a None since stays None (unbounded), only an existing Some(t) floor is raised to max(t, watermark+1). <!-- [^02745-111] -->

## Tailing-Feed REQ Narrowing Regression Fix

The #1281 lifecycle-aware fix regressed tailing-feed REQ narrowing; the correction honors the backfill intent (since=None stays unbounded) while keeping T129 for live feeds. <!-- [^02745-112] -->
