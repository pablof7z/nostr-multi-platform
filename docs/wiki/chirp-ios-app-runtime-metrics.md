---
title: Chirp iOS AppRuntimeMetrics — Debug Instrumentation Counters
slug: chirp-ios-app-runtime-metrics
summary: Debug-only instrumentation counters (nameRegressionCount, typedDecodeSuccessRate, emptyAfterNonEmptyCount) that quantify the Opus-defined reliability metrics in Chirp iOS.
tags:
  - ios
  - debug
  - instrumentation
  - testing
  - performance
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-26
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:54fc9b94-b995-46c6-8372-59c4abe0f95a
---

# Chirp iOS AppRuntimeMetrics — Debug Instrumentation Counters

> Debug-only instrumentation counters (nameRegressionCount, typedDecodeSuccessRate, emptyAfterNonEmptyCount) that quantify the Opus-defined reliability metrics in Chirp iOS.

## Overview

AppRuntimeMetrics is a debug-only instrumentation module added to Chirp iOS to quantify the reliability metrics (A1, A2, B1, C1, C3) defined in the Opus testing plan. It exposes counters that are readable at runtime for automated test assertions and manual debugging. [^4edd4-64]


The AppRuntimeMetrics module was delivered in PR #824 alongside two passing Swift unit tests for the profile display name fallback chain. The tests verify the priority order of claimedProfiles, mentionProfiles, and itemAuthorName in the resolveAuthorLabel helper. [^4edd4-99]
## nameRegressionCount

Counts the number of real-name → shortHex transitions for cache-resident profiles. Incremented each time an author display label changes from a resolved display name to a hex pubkey while the underlying kind:0 metadata is still in the kernel's resident store. This directly instruments Metric A2. Target value: always 0. [^4edd4-65]


The A2 name-regression counter is an honest upper-bound, not an exact count. Because no activeClaims field exists in the Swift snapshot, the counter cannot distinguish between on-screen claims and released claims. Every release/re-claim cycle may increment the counter even if no user-visible flicker occurs. With PR #823's structural fix (author_display_name baked into TimelineItem), the counter becomes a vanishing metric — the underlying flicker is structurally prevented regardless of the counter's value. [^4edd4-100]
## typedDecodeSuccessRate

Tracks the fraction of snapshot ticks where the typed NOFS decoder successfully decodes vs falls back to the generic (empty) path. Computed as successfulTypedDecodes / totalSnapshotTicks. This directly instruments Metric B1. Target: ≥99.9%. A drop below this threshold indicates a FlatBuffers schema mismatch like the primaryId camelCase bug. [^4edd4-66]

## emptyAfterNonEmptyCount

Counts transitions where the feed goes from having content (≥1 card) to empty (0 cards). Incremented each time a non-empty snapshot is followed by one with zero timeline items. This catches feed-blanking events during navigation, which are a symptom of claim churn where all profiles are released simultaneously on view disappear and not yet re-claimed on the next tick. [^4edd4-67]

## Debug-Only Gating

All AppRuntimeMetrics counters are compiled only in DEBUG builds. They must never ship in release configurations to avoid runtime overhead in production. The counters are read via a debug-only accessor on KernelModel, accessible to XCUITest targets through the test harness. [^4edd4-68]


kbLog.fault() with static string literals appears verbatim in Console.app on iOS device (not redacted as <private>), unlike NSLog with dynamic content. [^fe79b-4]

## update_frame_degradations_total

The Kernel tracks a monotonic `update_frame_degradations_total` counter over its lifetime for update-frame encoding/decoding degradations. [^54fc9-1]
## See Also

