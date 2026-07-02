---
title: "NMP Project Status: NIP Scope and ADR Spine"
slug: project-status
topic: project-status
summary: EPIC-NS-001 (#2340) is the governing p0 north-star epic for the clean-break NMP app architecture migration; all active slices trace back to it
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
---

# NMP Project Status: NIP Scope and ADR Spine

## Governing Epic

EPIC-NS-001 (#2340) is the governing p0 north-star epic for the clean-break NMP app architecture migration; all active slices trace back to it. The migration eliminates `register_defaults()`, raw `open_interest`, old projection tiers, and per-feature native ABI choices, replacing them with typed read sessions, a composable write door, an explicit composition root, and one UniFFI public surface for native. EPIC-NS-001 is approximately 70% complete, not ~100% as previously reported; migrate-readiness is approximately 60–65%.

The migration-readiness gate list before existing apps can move over is: product-read cutover (#2399, #2418), DX clean-room proof (#2256), and app-defined kinds first-class (#2408/#2413/#2414). App migration can begin before the entire EPIC-NS-001 epic closes; the M14 C-ABI deletion and perf-signal decision are epic-close cleanup that do not block the migrate-readiness gate (#2256).

<!-- citations: [^898a4-289cb] [^898a4-b541d] [^898a4-9a8e2] -->
## ADR Spine

The clean-break refactor is governed by ADR spine 0069–0073, applied to native,
browser, and starter targets in lockstep, with doctrine-lint and doc ratchets
locking each slice shut behind it. The ADR directory is current-only: obsolete
decision files are deleted after surviving rules move to their current owners.

<!-- citations: [^898a4-eaad2] [^3c942-d9519] [^898a4-bc2c6] -->
## Protocol Scope: NIP Status

NIP-57/zaps and NIP-47/NWC are formally post-v1; NIP-96 is never. Zap semantics are removed from the nmp-relations classifier. <!-- [^898a4-bfe17] -->

## Active Pre-V1 Workstreams

The profile-claim loop stash (#2298) is active pre-v1 correctness work, not post-v1 deferral. <!-- [^898a4-f7531] -->

NIP-29 owns only the h-tag routing concern, not kinds; the kinds filter was deliberately removed and GroupEventsProjection reads consumer-declared kinds. <!-- [^898a4-4d4bb] -->

The `nmp-native-runtime` extraction is incomplete and retains dual-ownership, so drift becomes a recurring tax when master moves fast and extractions leave the old path behind. <!-- [^3c942-83d7b] -->
