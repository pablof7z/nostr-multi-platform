---
title: Shared Snapshot Types — Public Types in nmp-app-chirp
slug: shared-snapshot-types
summary: Snapshot types (RelayStatus, ProfileCard, ActionResult, etc.) are public types in nmp-app-chirp, eliminating per-shell duplicate structs.
tags:
  - chirp
  - snapshot
  - types
  - architecture
  - cross-platform
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Shared Snapshot Types — Public Types in nmp-app-chirp

> Snapshot types (RelayStatus, ProfileCard, ActionResult, etc.) are public types in nmp-app-chirp, eliminating per-shell duplicate structs.

## Overview

Snapshot types that were previously re-declared in every shell are now public types in `nmp-app-chirp`. This eliminates the parallel struct sets: desktop had 18 local structs, TUI had its own parallel set, iOS had ~40 hand-rolled `Decodable` implementations. These are the A2 work item from the cross-platform parity plan. [^f3d8d-23]

## Public Types

The following types are now public in `nmp-app-chirp`: `RelayStatus`, `ProfileCard`, `ActionResult`, and other snapshot-related types. Rust shells (TUI, desktop) consume these directly. FFI shells (iOS, Android) will eventually consume them via codegen (F-05, post-v1). [^f3d8d-24]

## Confirmed Divergence Citations

Codex confirmed the snapshot divergence with exact citations: `chirp-desktop/snapshot.rs:24`, TUI `snapshot.rs:5` (`home_feed: Option<Value>`). These shell-local duplicate structs are deleted after migration to the shared types. [^f3d8d-25]


Public Types

TUI was migrated to use the shared A2 types from `nmp-app-chirp` in batch 3 (Sonnet agent, `tui-use-shared-types` task). This eliminated TUI's parallel struct set in favor of the canonical public types. [^f3d8d-48]
## See Also
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[chirp-client-typed-api|ChirpClient Typed API — Single Action Facade for All Shells]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide

