---
title: ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection
slug: adr-0025-bespoke-ffi-anti-pattern
summary: Apps must use nmp_app_register_snapshot_projection to ride the reactive push frame — minting bespoke pull-only FFI symbols (the ADR-0025 anti-pattern) forces polling and splits data across two channels.
tags:
  - architecture
  - ffi
  - reactivity
  - projection
  - adr
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection

> Apps must use nmp_app_register_snapshot_projection to ride the reactive push frame — minting bespoke pull-only FFI symbols (the ADR-0025 anti-pattern) forces polling and splits data across two channels.

## The Anti-Pattern: Bespoke Pull-Only FFI Symbols

ADR-0025 identifies the pattern of apps minting their own pull-only FFI symbols (e.g. `nmp_app_podcast_snapshot`) as an architectural anti-pattern that NMP exists to prevent. This creates a 'bespoke FFI cluster in the app binary' — the app goes around the framework instead of through it. [^d0690-6]

## The Correct Seam: register_snapshot_projection

The framework already ships the correct seam: `nmp_app_register_snapshot_projection(app, key, projector)` at `nmp-ffi/src/snapshot.rs:83`. A projection registered through this seam is appended to `KernelSnapshot::projections` on every tick and rides the reactive push frame via `make_update`, the same FlatBuffers `SnapshotFrame` that the listen callback delivers. It is not pull-only. [^d0690-7]

## How the Anti-Pattern Forces Polling

When an app creates a bespoke pull-only FFI symbol, the podcast projection data is not delivered over the reactive push channel. The app must then poll a rev counter (e.g. every 500ms) to detect changes — a direct violation of D8. The two channels split: push (reactive) carries kernel-level diagnostics like `store_open_failure` but fires rarely; pull (poll) carries all the UI data but is a poll. Had the projection been registered via `nmp_app_register_snapshot_projection`, it would ride the reactive push frame alongside everything else and there would never have been a poll. [^d0690-8]

## The Two-Channel Split Problem

NMP exposes two data channels: Push (reactive) — the generic kernel snapshot via `nmp_app_set_update_callback`, which carries identity projections, `store_open_failure`, and other kernel-level diagnostics. Pull (rev-counter poll) — the podcast projection via bespoke `nmp_app_podcast_snapshot` / `nmp_app_podcast_snapshot_rev` functions, carrying library/player/downloads/settings. When the podcast projection is not registered through the reactive push seam, these two channels remain split — the push channel carries diagnostics but fires rarely, while the pull channel carries UI data but requires polling. The fix is to unify them by registering the podcast projection through `nmp_app_register_snapshot_projection`. [^d0690-10]


## Framework Thesis Validation

The podcast-player app is framed as the "falsifiable test of the framework thesis" — it is the second NMP app (after Chirp) and represents a validation of whether the framework generalizes beyond Chirp. When a second app hits a gap, the first diagnostic question is: did the app bypass the framework, or is the framework missing a capability? The framework thesis holds when the answer is "use the seam that already exists" rather than "build new NMP plumbing." The podcast-player incident validated the thesis: the fix is app-side registration through the existing projection registry, not new NMP features. [^d0690-35]

## Known Bespoke Pull-Symbol Instances

Multiple bespoke pull-snapshot symbols exist across the codebase: `nmp_app_gallery_snapshot` (gallery, live at `apps/nmp-gallery/nmp-app-gallery/src/lib.rs:164` + header), `nmp_marmot_snapshot` (Chirp/Marmot), and the deprecated `nmp_app_chirp_snapshot` which has zero real callers left (only doc-comment mentions). The podcast-player's `nmp_app_podcast_snapshot` is the same pattern reborn downstream — a freshly-minted instance of exactly the anti-pattern ADR-0037 is deprecating. All must be driven to full removal, not left `#[deprecated]`; a half-landed migration is itself a violation. [^d0690-36]

`nmp_app_chirp_snapshot` is `#[deprecated]` and has zero real (non-comment) callers remaining — it should be removed immediately, not left half-deprecated. The removal is one of the safe `removeNow` items from the `snapshot-projection-cleanup` workflow. [^d0690-56]
## See Also
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[one-way-principle|One-Way Principle — Avoid Multiple Mechanisms for the Same Concern]] — related guide
- [[builder-guide-projection-docs-gap|Builder-Guide Documentation Gap — register_snapshot_projection Never Taught]] — related guide
- [[bespoke-pull-symbol-cleanup-workflow|Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide
- [[chirp-client-typed-api|ChirpClient Typed API — Single Action Facade for All Shells]] — related guide
- [[component-owned-reactivity-architecture|Component-Owned Reactivity Architecture]] — related guide
- [[architectural-compliance-verification-gate|Architectural Compliance Verification Gate — Verify Before Implementing]] — related guide
- [[account-operations-c-abi-symbols|Account Operations Must Use Bespoke C-ABI Symbols — Not dispatch_action]] — related guide
- [[resolved-profiles-kernel-projection|resolved_profiles — Kernel-Level Profile Merge Projection]] — related guide

