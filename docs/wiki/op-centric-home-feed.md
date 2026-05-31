---
title: OP-Centric Home Feed (V-80) — Architecture and Status
slug: op-centric-home-feed
summary: The OP-centric home feed (V-80) is live in Chirp as of 2026-05-29, replacing ModularTimelineProjection with OpFeedEngine + RootIndexedFeed + ActiveFollowSet.
tags:
  - feed
  - v80
  - op-feed
  - nmp-feed
  - nmp-nip01
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:ecf13381-c8ef-40bf-9498-04a1d1f2af8f
---

# OP-Centric Home Feed (V-80) — Architecture and Status

> The OP-centric home feed (V-80) is live in Chirp as of 2026-05-29, replacing ModularTimelineProjection with OpFeedEngine + RootIndexedFeed + ActiveFollowSet.

## What Landed (V-80)

The OP-centric home feed is **complete** as of 2026-05-29. All 7 rungs landed:
- **B1** — typed schema/encoder
- **B2** — chirp-tui decoder
- **B3** — iOS decoder
- **B4** — Android decoder
- **V-82** and **V-83** (repost hydration)

Chirp is live on the new feed. The `ModularTimelineProjection` is gone; `ChirpHandle.snapshot()` now delegates directly to `OpFeedEngine::snapshot()`. [^42908-9]


The OP feed generated three iOS Swift bridge files: `ContentTree.generated.swift`, `OpFeedSnapshot.generated.swift`, and `FeedWindow.generated.swift`. These were added to disk by PRs #755 and #762 but the `project.pbxproj` was not regenerated via `xcodegen generate`, causing build failures until regeneration was performed. [^9a2c7-30]

Desktop OP-feed cutover landed in the cross-platform fix batch (A3-desktop-feed): `decode_snapshot_with_typed` + `nmp_nip01::OP_FEED_SCHEMA_ID` sidecar extraction, following the TUI pattern. Desktop had been on FlatBuffers transport already but was ignoring `nmp.feed.home`/NOFS data, rendering `snap.items` instead. [^f3d8d-34]
## Core Artifacts

- `crates/nmp-feed/src/root_indexed.rs` — `RootIndexedFeed` engine
- `crates/nmp-nip01/src/op_feed/wiring.rs` — `pub fn register_op_feed`
- `crates/nmp-nip02/src/active_follow_set.rs` — `ActiveFollowSet`
- `crates/nmp-app-template/src/op_feed_defaults.rs` — `register_op_feed_defaults` (exported from lib.rs)
- `apps/chirp/nmp-app-chirp/src/ffi/register.rs:134` — calls `nmp_app_template::register_op_feed_defaults` [^42908-10]

## ADRs

- **ADR-0035** — generic-root-indexed-feed-engine (RootIndexedFeed)
- **ADR-0036** — composition-root-followset-expansion (ActiveFollowSet composition; replaced V-45/LogicalInterest::SocialTimeline)
- **ADR-0037** — typed-flatbuffers-runtime-projections (V-80 spec)
- **ADR-0038** — typed-op-feed-projection (typed NOFS schema)

All four ADRs exist with no gaps. [^42908-11]

## Follow-Set Composition Pattern

The host (composition root) declares follow-set kinds via `OpenContactListSubscription { kinds: {...} }`. This sets `follow_feed_kinds` in the kernel, which gates emission of `projections.timeline` in the snapshot. A snapshot key is only emitted when the corresponding view is open (D5 doctrine). Tests and shells that expect `projections.timeline` must open a contact-list subscription with the relevant kinds before ingesting events. [^42908-12]


What Landed (V-80)

Desktop has a remaining post-V80 rendering gap: the app reads snap.items which is always empty after V-80, while notes are delivered via projections[nmp.feed.home] typed sidecar. The fix requires backfilling items (and active_account, profile, accounts) from the projections map, not just the FlatBuffers typed sidecar decode. [^ecf13-27]
## See Also
- [[chirp-ios-nmp-gallery-component-adoption|Chirp iOS NMP Gallery Component Adoption — Gap Audit and Implementation Plan]] — related guide
- [[xcodegen-project-regeneration|XcodeGen Project Regeneration — Never Hand-Edit project.pbxproj]] — related guide
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[desktop-kernel-snapshot-projection-backfill|Desktop KernelSnapshot Projection Backfill — Fields Are in projections, Not Top-Level]] — related guide
- [[claim-expansion-terminate-claim-invariant|Claim Expansion — terminate_claim Is the Sole Phase::Terminal Transition Point]] — related guide

