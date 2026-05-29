---
title: ChirpClient Typed API — Single Action Facade for All Shells
slug: chirp-client-typed-api
summary: ChirpClient is the typed Rust client API in nmp-app-chirp that replaces per-shell hand-rolled JSON action envelopes with typed method calls.
tags:
  - chirp
  - api
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

# ChirpClient Typed API — Single Action Facade for All Shells

> ChirpClient is the typed Rust client API in nmp-app-chirp that replaces per-shell hand-rolled JSON action envelopes with typed method calls.

## Overview

`ChirpClient` is the typed Rust client API struct in `nmp-app-chirp`. It replaces the per-shell pattern of hand-rolling JSON action-envelope strings (e.g., `json!({"PublishNote":…})`). Shells call typed methods like `chirp.publish_note(content)` instead of constructing JSON literals. This is the A1 work item from the cross-platform parity plan. [^f3d8d-19]

## Methods

`ChirpClient` provides typed methods for all Chirp actions: publish, react, follow, DM, zap, and account operations. It backs all C-ABI symbols, serving as the single Rust-side action facade for every platform shell. [^f3d8d-20]

## FFI Constraint

Do NOT add per-verb C symbols to the FFI boundary for `ChirpClient`. Per `docs/plan.md:125`, new bespoke FFI symbols are frozen. The typed API is consumed directly by Rust shells (TUI, desktop) and backs the existing generic C-ABI `dispatch_action` symbol for FFI shells (iOS, Android). [^f3d8d-21]

## Migration Path

Rust shells (TUI, desktop) migrate to call `ChirpClient` directly instead of hand-rolling JSON action envelopes. The `desktop-use-chirpclient` task migrates `chirp-desktop` bridge to use `ChirpClient`; `tui-use-shared-types` migrates TUI. FFI shells continue to use the generic `dispatch_action` C-ABI symbol which `ChirpClient` backs internally. [^f3d8d-22]


Migration Path

Desktop migration uses pure free functions (`publish_note_action`, `react_action`, etc.) from `nmp-app-chirp/typed_api.rs` rather than a `ChirpClient` struct field on `AppRuntime`. A struct field creates a raw pointer lifetime problem because FFI callback registration stores raw pointers that must outlive the registration — and a `ChirpClient` field on `AppRuntime` would be dropped before the FFI layer if not carefully managed. Pure free functions have no state and avoid this issue entirely. Shells call them to build action JSON and dispatch through the existing generic `dispatch_action` path. This pattern is now the canonical approach for Rust shells: use the pure action-builder free functions, not a `ChirpClient` instance. [^f3d8d-45]
## See Also
- [[chirp-cross-platform-parity-plan|Chirp Cross-Platform Parity — Plan, Root Causes, and Ordered Work]] — related guide
- [[shared-snapshot-types|Shared Snapshot Types — Public Types in nmp-app-chirp]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[multi-agent-integration-workflow|Multi-Agent Integration Workflow — Fan-Out with Integration Branch]] — related guide

