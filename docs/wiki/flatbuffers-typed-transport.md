---
title: FlatBuffers Typed Transport — Hybrid Migration Architecture
slug: flatbuffers-typed-transport
summary: SnapshotFrame carries a mandatory generic FlatBuffers Value payload plus optional typed-projection sidecars; the goal is to make the generic payload optional once all consumers emit typed schemas.
tags:
  - flatbuffers
  - transport
  - nmp-core
  - adr-0037
  - adr-0038
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# FlatBuffers Typed Transport — Hybrid Migration Architecture

> SnapshotFrame carries a mandatory generic FlatBuffers Value payload plus optional typed-projection sidecars; the goal is to make the generic payload optional once all consumers emit typed schemas.

## Current State

The transport layer uses a hybrid approach:

- **Primary (mandatory)**: A generic FlatBuffers `Value` tree (JSON-like primitive model) is the mandatory payload in every `SnapshotFrame` (`payload:Value` in `crates/nmp-core/schema/nmp_update.fbs`). The comment marks it "compatibility during migration."
- **Sidecars (optional)**: Typed FlatBuffers schemas ship as optional typed-projection sidecars via the `typed_projections` vector on `SnapshotFrame` (added in commit d73e048b, PR #582). [^42908-13]

## Deployed Typed Schemas

- `nmp.nip01.ModularTimelineSnapshot` (NFTS) — `crates/nmp-nip01/schema/timeline_snapshot.fbs`
- `nmp.nip01.OpFeedSnapshot` (NOFS) — `crates/nmp-nip01/schema/op_feed.fbs`
- `nmp.feed.window.FeedWindow` (NFWM) — `crates/nmp-feed/schema/feed_home.fbs`
- `nmp.content.tree` (NFCT) — various embedded projections [^42908-14]

## Migration Path

The target state is for `payload:Value` to become optional (nullable) once all protocol/generic projections emit typed FlatBuffers exclusively. This is currently blocked on the remaining generic projection consumers (non-feed namespaces). There are no `FullState` or `ViewBatch` typed root types. [^42908-15]

## Rule: No Production JSON Runtime Fallback

The architecture doctrine states there must be no production JSON runtime fallback. The generic `Value` tree is the primary backward-compatible generic interchange, not a fallback — it coexists with typed sidecars in the same frame. The forward direction is for typed schemas to replace it once all consumers are migrated. [^42908-16]

## See Also
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[adr-0037-typed-projections-status|ADR-0037 — Typed FlatBuffers Runtime Projections (Proposed, Hot-Path Only)]] — related guide

