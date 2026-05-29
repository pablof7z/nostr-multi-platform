---
title: ADR-0037 — Typed FlatBuffers Runtime Projections (Proposed, Hot-Path Only)
slug: adr-0037-typed-projections-status
summary: ADR-0037 is Proposed (not shipped in v0.1.0), is scoped as a hot-path performance optimization only, and is not required to eliminate polling — the generic register_snapshot_projection seam already delivers projections reactively.
tags:
  - adr
  - flatbuffers
  - projection
  - architecture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# ADR-0037 — Typed FlatBuffers Runtime Projections (Proposed, Hot-Path Only)

> ADR-0037 is Proposed (not shipped in v0.1.0), is scoped as a hot-path performance optimization only, and is not required to eliminate polling — the generic register_snapshot_projection seam already delivers projections reactively.

## Status

ADR-0037 (typed-flatbuffers-runtime-projections) shipped and is live at HEAD. The ADR-0038 rollout is marked complete, with V-84 (iOS, PR #762), V-85 (Android, PR #764), and V-86 (CI, PR #781) all verified at HEAD (`BACKLOG.md:1795, 1900-1902`). The FlatBuffers transport envelope is the mandatory primary transport (`F-10`, `BACKLOG.md:1786`). The ADR doc may show `Status: Proposed` due to doc-status lag — the feature is live. The assistant's initial claim that "ADR-0037 didn't ship" was incorrect; that was a misreading of doc-status lag as feature absence.

<!-- citations: [^d0690-11] [^d0690-27] [^d0690-38] -->
## Scope and Intent

ADR-0037 introduces a typed FlatBuffers sidecar mechanism (`register_typed_snapshot_projection` / `decode_snapshot_with_typed`). It is a hot-path performance optimization for projections that re-serialize large volumes of data every tick (e.g. the Chirp home feed). The typed sidecar is an additive, per-key optimization rolled out by coordinated cross-host migration — never chosen by an individual app. Apps always get the generic baseline emission by default; typed sidecars are added per-key through schema + platform decoder + CI pin coordination.

<!-- citations: [^d0690-12] [^d0690-28] -->
## Not the Keystone for Podcast-Player

The podcast-player's projection (library, now-playing, settings) does not need ADR-0037 to eliminate its 500ms poll. The correct fix is to register the podcast projection through the existing `nmp_app_register_snapshot_projection` seam, which already delivers projections reactively over the push frame. The agent's claim that ADR-0037 is the "keystone" was wrong — reaching for the typed sidecar confuses an additive performance optimization with the fundamental wiring fix. The podcast-player's root error was creating a bespoke pull symbol, not failing to use typed FlatBuffers.

<!-- citations: [^d0690-13] [^d0690-29] -->
## The 'One Way' Principle

The no-dual-seam test is about registry-vs-bespoke-pull — not generic-vs-typed. NMP has one canonical way to publish a projection: register it through the projection registry (either `register_snapshot_projection` or `register_typed_snapshot_projection` — both are live, neither is deprecated). Apps always get the generic baseline emission by default with no decision required. Typed sidecars are additive per-key optimizations coordinated cross-host. An app never chooses an encoding; the encoding is chosen at the framework level by migration coordination. The podcast-player's bespoke `nmp_app_podcast_snapshot` pull symbol is an illegal second path — it is an instance of exactly the bespoke per-app snapshot symbol that ADR-0037 is deprecating.

<!-- citations: [^d0690-14] [^d0690-30] -->
## See Also
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[one-way-principle|One-Way Principle — Avoid Multiple Mechanisms for the Same Concern]] — related guide
- [[flatbuffers-typed-transport|FlatBuffers Typed Transport — Hybrid Migration Architecture]] — related guide

