---
title: One-Way Principle — Avoid Multiple Mechanisms for the Same Concern
slug: one-way-principle
summary: NMP prescriptively avoids having multiple ways of doing the same thing. When an existing generic mechanism solves a problem, introducing a second specialized mechanism is rejected.
tags:
  - architecture
  - principle
  - design
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# One-Way Principle — Avoid Multiple Mechanisms for the Same Concern

> NMP prescriptively avoids having multiple ways of doing the same thing. When an existing generic mechanism solves a problem, introducing a second specialized mechanism is rejected.

## Core Principle

NMP has a prescriptive architectural goal: avoid having multiple ways of doing the same thing. The primary axis for testing this is registry-vs-bespoke-pull: there is one canonical way to publish a projection — register it through the projection registry. Any bespoke pull-only FFI symbol that an app mints to extract projection data is an illegal second path. This is not about generic-vs-typed encoding; both `register_snapshot_projection` and `register_typed_snapshot_projection` are part of the same canonical registry seam. The encoding is chosen at the framework level by coordinated cross-host migration, never by an individual app.

<!-- citations: [^d0690-23] [^d0690-31] -->
## Concrete Application: Projection Delivery

There must be one mechanism for delivering projections to app shells: the projection registry. Both generic `Value` tree registration and typed FlatBuffers registration go through this same seam. An app never chooses between generic and typed — it always registers its projection and gets the generic baseline emission by default. Typed sidecars are added per-key by coordinated cross-host migration (schema + iOS + Android + tui decoders + CI pins), never by an individual app's decision. Bespoke pull-only FFI symbols (`nmp_app_*_snapshot`) are the illegal second path that the registry seam exists to prevent.

<!-- citations: [^d0690-24] [^d0690-32] -->
## Enforcement

When reviewing architectural proposals, flag any approach that introduces a second code path for an already-solved concern. The correct question is: 'Does the existing mechanism already solve this?' If yes, the new mechanism is rejected regardless of its other merits. [^d0690-25]

## See Also
- [[adr-0037-typed-projections-status|ADR-0037 — Typed FlatBuffers Runtime Projections (Proposed, Hot-Path Only)]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[builder-guide-projection-docs-gap|Builder-Guide Documentation Gap — register_snapshot_projection Never Taught]] — related guide
- [[nmp-gallery-cross-platform-consolidation|NMP Gallery Cross-Platform Consolidation — Registry-Driven Component Catalog]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide

