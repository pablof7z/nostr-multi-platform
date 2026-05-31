---
title: Session Recap — Cross-Platform Parity, Reliability, and Backlog Wave
slug: session-2026-05-backlog-wave-complete
summary: "Complete recap of the session: cross-platform parity fixes, reliability testing suite, and backlog wave with 8 dispatched items, 15 total PRs merged."
tags:
  - session-recap
  - backlog
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Session Recap — Cross-Platform Parity, Reliability, and Backlog Wave

> Complete recap of the session: cross-platform parity fixes, reliability testing suite, and backlog wave with 8 dispatched items, 15 total PRs merged.

## Session Overview

The session spanned two phases: first, cross-platform parity fixes (crash, repost, Android); then a backlog dispatch wave that was course-corrected after the user demanded proper prioritization. The first wave (8 items, incorrectly optimized for parallelizability over priority) landed on master at 1e3159f7. The second, correctly-prioritized wave (5 Section-1 items, ranked by the backlog's own severity labels) landed on master at bb8bc105. Total: 15+ PRs merged across both phases.

<!-- citations: [^4edd4-132] [^4edd4-171] -->
## PRs From Earlier Session (Merged First)

PR #810 (kernel panic + FlatBuffers primaryId fix), PR #811 (iOS repost NIP-18), PR #814 (Android Relays tab duplicate-key crash). Plus PRs #821 (Rust warm-reclaim tests), #822 (UI regression tests + perf gates), #823 (structural flicker fix — author_display_name in TimelineItem), #824 (Swift instrumentation + unit tests). These were merged before the backlog wave dispatch began. [^4edd4-133]

## Backlog Wave PRs

8 items from BACKLOG.md were dispatched via Opus prioritization (though incorrectly optimized for parallelizability over priority): #825 V-68 orphan ingest file deletion, #826 V-103 D1 offline-bootstrap test (blocked, reworked, then merged), #827 V-57-S2 router kind constants (nip59 half correctly escalated due to dependency cycle), #828 V-59 EventStore clock injection, #829 V-89 sentinel double-stamping removal, #830 V-100 NWC validation Swift-to-Rust, #831 V-105 typed test observables (changes-requested, cleaned up, then merged), #832 V-104 e2e pipeline tests (fake negentropy replaced with real T129 watermark mechanism). [^4edd4-134]

## Open Follow-Ups

V-57-S2 nip59 kind migration (dependency-cycle owner decision needed), V-60 LMDB LRU eviction (unblocked by V-59), monotonic_rev test remains #[ignore]'d pending milestones M2/M3/M8, and the genuinely top-priority HIGH items (V-68 Stage 2, V-87, V-90, V-52) were not dispatched because parallelizability was incorrectly used as the primary filter. [^4edd4-135]



After the second wave, the session continued with the user choosing to resolve the V-90 blocker: draft ADR-0040 (capability-worker seam). ADR-0040 was drafted as PR #842, fact-checked (off-by-one identity.rs citations corrected from 826,864,1019 to 825,863,1018), and merged as Proposed per user decision (option 3: merge as Proposed, defer implementation). This moves V-90 from 'ADR-gated with no actionable path' to 'design ratifiable, implementation plan ready' — three independently-shippable implementation PRs (DM off-actor, cold-start PendingSign, capability-worker seam last) are deferred until explicitly greenlit. The ADR supersedes ADR-0024 for the native capability class. Additionally, the user provided live-relay tooling (nak serve in-memory relay, relay.primal.net) unblocking the F-02/F-04 verification harness when that work begins. [^4edd4-195]
## Correctly-Prioritized Second Wave (After Course Correction)

After the user's correction, the backlog was re-prioritized by Opus with the correct instruction: rank by severity labels, Section 1 HIGH first, parallel-safety only as tiebreaker. Five Section-1 items landed: V-52 single-relay browsing (HIGH · v1 DX, PR #836), V-42 NIP-51 mute list (HIGH · v1-A safety, PR #834), V-87 D1 startup kernel half (HIGH · D1, PR #835), V-68-S2 thread half D0 kind externalization (HIGH · D0, PR #840), and V-60 LMDB LRU eviction (MEDIUM · topmost unblocked, PR #841). Every PR was Sonnet-review-gated; the gate caught and forced fixes for a rev-collision protocol defect (#835), a cross-account mute leak (#834), and a release-manifest CI block (#834). Master landed at bb8bc105. [^4edd4-172]

The second wave also included V-60 LMDB LRU eviction (MEDIUM, PR #841), dispatched as the topmost unblocked Section-1 item after the parallel-safe HIGH set was exhausted. V-60 was only actionable because V-59's clock injection (merged earlier) provided the prerequisite coverage(now_secs) access. The implementation extended GcBudget with a ceiling, threaded the kernel clock into gc_step, added LRU access tracking to both backends (mem HashMap, LMDB sub-db + AtomicU64 seq), and ensured eviction skips pinned/claimed events and avoids tombstoning so evicted events stay re-fetchable. Review confirmed: eviction cleans every secondary index, pinned events provably survive, the write-on-read is safe and bounded to point-reads. [^4edd4-194]
## See Also

