---
title: Chirp iOS Reliability Metrics and Testing Plan
slug: chirp-ios-reliability-metrics-testing-plan
summary: Opus-defined reliability metrics (A1, A2, B1, C1, C3) and 4-tier testing plan to quantify and eliminate flicker, slowness, and instability in Chirp iOS.
tags:
  - ios
  - performance
  - testing
  - reliability
  - metrics
  - opus-plan
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Chirp iOS Reliability Metrics and Testing Plan

> Opus-defined reliability metrics (A1, A2, B1, C1, C3) and 4-tier testing plan to quantify and eliminate flicker, slowness, and instability in Chirp iOS.

## Overview

An Opus agent produced a comprehensive reliability measurement and testing framework for Chirp iOS. The framework defines 5 key metrics with hard targets and a 4-tier testing plan ordered by leverage. The goal is to quantify and systematically eliminate the flicker, slowness, and instability issues observed during navigation and scrolling. [^4edd4-53]


The testing plan was executed across four PRs: #821 (Rust Tier-0 invariant tests — 10 passing), #822 (UI regression tests + perf gates), #823 (structural flicker fix — author_display_name baked into TimelineItem), and #824 (Swift instrumentation + 2 passing unit tests for the fallback chain). [^4edd4-101]
## Metric A1 — Warm-Reclaim Name Gap

Definition: The number of ticks where a claimed, cache-resident author shows shortHex instead of their display name. Target: 0 ticks. Current state: ~1–2 ticks per back-navigation. This is the primary metric for the profile name flicker bug. A value of 0 means no user-visible degradation to hex pubkey for any profile whose kind:0 is already cached in the resident store. [^4edd4-54]

## Metric A2 — Name Regression Count

Definition: The number of real-name → shortHex transitions that occur while the kind:0 metadata is cached in the resident store. Target: 0 (hard gate). Current state: Non-zero on every back-navigation. A name regression is defined as a single author's display label transitioning from a resolved display name back to a hex pubkey while the underlying kind:0 event remains in the kernel's resident store. This must never happen. [^4edd4-55]

## Metric B1 — Typed Decode Success Rate

Definition: The fraction of snapshot ticks where the typed NOFS decoder succeeds vs falls back to the generic (empty) decode. Target: ≥99.9%. Current state: Unmeasured. This metric catches FlatBuffers schema mismatches like the primaryId camelCase bug that caused every card to fall back to an empty decode. A drop below 99.9% indicates a serialization or schema drift bug. [^4edd4-56]

## Metric C1 — Snapshot Apply p99 Latency

Definition: The 99th percentile of callbackToApplied latency — time from snapshot emission to UI application. Target: ≤16ms p99, ≤50ms ceiling. Current state: A 50ms gate exists, but p99 is unmeasured. This metric catches kernel→UI bridge bottlenecks. The 50ms ceiling is a hard maximum; any tick exceeding it indicates a blocking operation on the snapshot apply path. [^4edd4-57]

## Metric C3 — Idle Re-Render Rate

Definition: The number of SwiftUI body evaluations per second while content is unchanged. Target: 0 for unchanged rows. Current state: ~4 full-tree invalidations per second at the 4Hz tick rate, even when no data has changed. This means every snapshot tick triggers a re-render of every row in the feed, regardless of whether the data for that row changed. The target is that only rows whose underlying data actually changed should re-render. [^4edd4-58]

## Testing Plan — Tier 0 (Rust Unit Tests)

Highest leverage, fastest execution. Three tests: (1) warm_reclaim_reemits_profile_next_tick_with_no_req — claims a pubkey, ingests its kind:0, releases the claim, re-claims, then asserts the next tick has display_name non-null and zero pending REQ. This definitively settles whether the gap involves relay round-trips. (2) claimed_profiles_present_iff_claim_held — pins the release lifecycle: a profile must be present in claimed_profiles when and only when it is currently claimed. (3) claim_release_reclaim_does_not_lose_resident_profile — multi-consumer guard: if consumer Y holds a claim while consumer X releases, the profile must remain in claimed_profiles. These tests landed in PR #821 and confirmed that warm re-claims require zero relay REQ. [^4edd4-59]


These tests landed in PR #821. Critical finding confirmed: claim_profile short-circuits with zero relay REQ for already-resident pubkeys, proving the flicker gap is 100% Swift-side lifecycle churn — not a missing network fetch. [^4edd4-102]
## Testing Plan — Tier 1 (Swift Unit Tests)

Two tests: (1) profile_forPubkey_fallback_chain — asserts the priority order claimedProfiles > mentionProfiles > nil exactly, documenting that mentionProfiles is the fallback for claimedProfiles absence. (2) noteRow_authorDisplayLabel_priority — documents that the NOFS eventCards.authorDisplayName gap-filler is load-bearing: when claimedProfiles and mentionProfiles are both empty, the authorDisplayName from the event card is the last resort before shortHex. [^4edd4-60]


Delivered in PR #824. Two tests: one for the fallback chain priority and one asserting the itemAuthorName rung's precedence (added during the rebase onto PR #823). Both pass. [^4edd4-103]
## Testing Plan — Tier 2 (XCUITest)

Two UI tests using the NMP_TEST_NSEC harness: (1) profileName_persists_through_nav_roundtrip — waits for a display name to resolve, pushes to the profile view, pops back, and asserts the label never matches the hex pubkey regex /^[0-9a-f]{4}…[0-9a-f]{4}$/ after the first resolution. This is the direct regression test for the flicker complaint. (2) feed_does_not_blank_during_nav — asserts the timeline retains at least 1 item during a Settings→Home round-trip, catching feed blanking during navigation. [^4edd4-61]


Delivered in PR #822. The profileName_persistsThroughNavRoundtrip test uses the regex /^[0-9a-f]{4}…[0-9a-f]{4}$/ to detect hex pubkey fallbacks — note that shortHex uses a Unicode ellipsis (…, U+2026), not ASCII dots (...). The test also required adding a timeline-author-name accessibility ID to NoteRowView.swift for reliable element lookup. [^4edd4-104]
## Testing Plan — Tier 3 (Performance Tests)

Three performance gates using XCTest metrics: (1) Scroll FPS — XCTOSSignpostMetric.scrollDecelerationMetric gate: ≥58fps with hitch <5ms/s. (2) Nav transition — .navigationTransitionMetric gate: 0 dropped frames on push/pop transitions. (3) Idle re-render — Instruments SwiftUI View Body count: row bodies must not re-evaluate on no-op ticks. The baseline from docs/perf/reactivity-bench/2026-05-17-run-001.md shows 98% false-wake rate at idle and 49% at scroll. [^4edd4-62]

## Testing Plan — Tier 4 (Maestro E2E)

End-to-end test against a local relay: a known-seed pubkey's display name must persist through navigate-away/back cycles. Maestro drives the full app (not just a test harness) and validates the complete kernel→UI pipeline against a controlled relay environment. [^4edd4-63]

## See Also
- [[profile-flicker-warm-reclaim-gap|Profile Name Flicker — Warm-Reclaim Lifecycle Gap]] — related guide

