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
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Watermark Removal

## Watermark Removal

The live since-floor for REQ subscriptions is derived from store content (newest matching event per author/coord/tag) via watermark_fn, not from persisted WatermarkRow.synced_up_to — which has zero production writers. The persisted watermark machinery (WatermarkKey/WatermarkRow/SyncMethod/Coverage, read_watermark/write_watermark/coverage trait methods, LMDB watermarks sub-db) has been deleted as dead code.

<!-- citations: [^02745-109] [^02745-121] [^02745-136] -->
## T129 Watermark Rewrite and Rule 1 Reversal

R4 defects 2 (T129 watermark rewrite) and 4 (Rule 1 wildcard absorption) were documented, tested features, not bugs; their reversal was escalated as owner decisions (#1281, #1282) rather than merged autonomously. <!-- [^02745-110] -->

## since=None Backfill Exemption

Issue #1281's since=None backfill semantics is decided: since=None is exempted from the T129 watermark rewrite for non-Tailing (backfill/OneShot) subscriptions, while Tailing subscriptions keep narrowing to watermark+1. For a non-Tailing interest, a None since stays None (unbounded); only an existing Some(t) floor is raised to max(t, watermark+1). Tailing interests always apply T129 watermark narrowing (since=watermark+1).

<!-- citations: [^02745-122] [^02745-137] -->
#1281's since=None backfill semantics is the single genuinely-needs-owner decision — whether to exempt unbounded (since=None) interests from the T129 watermark rewrite. since=None interests are exempted from the T129 watermark rewrite: a None since stays None (unbounded), only an existing Some(t) floor is raised to max(t, watermark+1). <!-- [^02745-111] -->

## Tailing-Feed REQ Narrowing Regression Fix

The #1281 lifecycle-aware fix regressed tailing-feed REQ narrowing; the correction honors the backfill intent (since=None stays unbounded) while keeping T129 for live feeds. <!-- [^02745-112] -->
