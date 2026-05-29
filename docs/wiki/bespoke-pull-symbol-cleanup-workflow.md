---
title: Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow
slug: bespoke-pull-symbol-cleanup-workflow
summary: The four-phase evaluate→verify→plan→fix fan-out workflow that drives bespoke pull-symbol deprecations to full removal without breaking master.
tags:
  - workflow
  - cleanup
  - deprecation
  - projection-registry
  - fan-out
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow

> The four-phase evaluate→verify→plan→fix fan-out workflow that drives bespoke pull-symbol deprecations to full removal without breaking master.

## Workflow Shape

The bespoke pull-symbol cleanup workflow (`snapshot-projection-cleanup`, `wdox1fu6v`) uses a four-phase fan-out pattern designed to prevent breaking master while driving half-landed deprecations to full removal. [^d0690-43]

## Phase 1: Evaluate (4 parallel agents)

Inventory agent — enumerates every bespoke `*_snapshot` pull symbol: its exporter, C header declaration, deprecation status, and real (non-comment) callers. Classifies each as dead, live, or unknown. Canonical seam agent — traces `register_snapshot_projection` end-to-end (register → `KernelSnapshot::projections` → pushed FlatBuffers frame → host reads `projections[key]`) and finds a host already doing it right as a copy-paste exemplar. Doc gap agent — confirms the builder-guide never teaches the seam, pins which chapters should, and outlines the positive 'How to add a projection' section. Dual-emission agent — evaluates whether the staged-removal trigger is met for any key in the generic-Value vs typed-sidecar migration. [^d0690-44]

## Phase 2: Verify (Adversarial Gate)

Every symbol classified as 'dead' or 'unknown' gets an adversarial agent whose job is to prove it still has a caller. A symbol is only marked safe-to-remove if that agent fails to find any real caller. This gate prevents a removal PR from breaking master by deleting a symbol with an overlooked callsite. [^d0690-45]

## Phase 3: Plan (Opus Architect)

Produces an ordered completion plan: `removeNow` (verified dead — e.g., `nmp_app_chirp_snapshot` with zero real callers) vs `migrateFirst` (live consumers like `nmp_app_gallery_snapshot` and `nmp_marmot_snapshot` need migrating to the canonical seam before removal), plus the doc spec for the builder-guide projection section. [^d0690-46]

## Phase 4: Fix (Worktree-Isolated PRs)

Opens PRs for the two safe items: the positive projection-docs guide, and removal of verified-dead symbols. Live-consumer migrations (gallery, Marmot) come back as a sequenced plan for deliberate human review and dispatch — agents never blind-migrate a shell. The podcast-player repo is out of scope (separate repository, corrective message sent to that agent instead). [^d0690-47]

## Deliberate Restraints

The workflow deliberately does NOT autonomously: migrate live gallery/marmot consumers (real shell changes, gated on human review of the plan), or touch the podcast-player repo (separate repo, corrective message handles it). These are explicit design choices to prevent automated breakage of production shells. [^d0690-48]

## See Also
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[half-landed-migration-is-not-done|A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide
- [[half-landed-migration-is-not-done|A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations]] — related guide

