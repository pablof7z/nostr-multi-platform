---
title: V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam
slug: v-107-bespoke-snapshot-consumer-migration
summary: "Backlog item V-107: migrate live bespoke pull-snapshot consumers (gallery, Marmot) to the canonical register_snapshot_projection pushed-frame seam, gated on V-37 push-vs-pull resolution and human review."
tags:
  - backlog
  - v-107
  - projection-registry
  - migration
  - anti-pattern
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam

> Backlog item V-107: migrate live bespoke pull-snapshot consumers (gallery, Marmot) to the canonical register_snapshot_projection pushed-frame seam, gated on V-37 push-vs-pull resolution and human review.

## Scope

V-107 tracks the migration of two live bespoke pull-snapshot consumers onto the canonical `register_snapshot_projection` → pushed-frame seam, followed by removal of the bespoke symbols. The consumers are: `nmp_app_gallery_snapshot` (gallery, live at `apps/nmp-gallery/nmp-app-gallery/src/lib.rs:164` + header) and `nmp_marmot_snapshot` (Chirp/Marmot). [^d0690-49]

## Root Cause Linkage

V-107 is directly tied to the podcast-player incident: apps keep reinventing the pull+poll anti-pattern because the canonical `register_snapshot_projection` seam is undocumented in the builder-guide. The doc fix is in-flight via the `snapshot-projection-cleanup` workflow. [^d0690-50]

## Architectural Decision Required

V-37's ADR frames the need as a generic pull path, but the podcast-player incident is evidence that the right answer is the push registry. The ADR must resolve push-vs-pull before this migration starts. The `snapshot-projection-cleanup` workflow is producing the V-37 push-vs-pull decision as a prerequisite. [^d0690-51]

## Gating

Explicitly gated on human review — agents must not autonomously migrate gallery/marmot consumer shells. The `snapshot-projection-cleanup` workflow returns the verified migration plan that feeds V-107. [^d0690-52]

## Linked Items

V-37 (ADR: push-vs-pull architecture), PD-039, PD-041, V-87 item 4. Committed to master at `c0302ff9`, pathspec-scoped to `docs/BACKLOG.md` only. [^d0690-53]

## Priority

Prioritized for team-wide awareness. The podcast-player incident proves this is not theoretical — bespoke pull symbols are being freshly minted in new downstream apps because the positive path is undocumented. [^d0690-54]

## See Also
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[bespoke-pull-symbol-cleanup-workflow|Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow]] — related guide
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[half-landed-migration-is-not-done|A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[resolved-profiles-kernel-projection|resolved_profiles — Kernel-Level Profile Merge Projection]] — related guide

