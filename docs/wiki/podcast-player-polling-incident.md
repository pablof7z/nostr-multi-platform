---
title: Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern
slug: podcast-player-polling-incident
summary: The podcast-player app (second NMP app) fell into the ADR-0025 anti-pattern by creating a bespoke pull-only FFI symbol, forcing a 500ms D8-violating poll. The fix uses the existing register_snapshot_projection seam, not ADR-0037.
tags:
  - podcast-player
  - d8
  - adr-0025
  - adr-0037
  - ffi
  - incident
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern

> The podcast-player app (second NMP app) fell into the ADR-0025 anti-pattern by creating a bespoke pull-only FFI symbol, forcing a 500ms D8-violating poll. The fix uses the existing register_snapshot_projection seam, not ADR-0037.

## The 500ms Poll

The podcast-player app contains a `KernelModel.startSnapshotPoll()` method (`App/Sources/Bridge/KernelModel.swift:184`) that runs a `Task` polling every 500ms: `try? await Task.sleep(for: .milliseconds(500))` followed by `pullPodcastSnapshotIfChanged()`. This is a direct D8 violation. [^d0690-18]

## Root Cause

The poll exists because the app created its own bespoke pull-only FFI symbol (`nmp_app_podcast_snapshot`) instead of using the framework's `nmp_app_register_snapshot_projection` seam. The C header exposes three functions for it: `nmp_app_podcast_snapshot` (pull the JSON), `nmp_app_podcast_snapshot_rev` (cheap atomic rev counter), and `nmp_app_podcast_snapshot_free`. There is no push callback for podcast data because the projection was never registered through the reactive seam. This is the ADR-0025 anti-pattern. [^d0690-19]

## Two-Channel Split Consequence

The bespoke FFI creates two channels that are split incorrectly: Push (reactive) via `nmp_app_set_update_callback` — carries generic `KernelUpdate` (identity projections, `store_open_failure`, etc.), fires rarely, didn't fire at all on a bare launch in testing. Pull (500ms poll) via bespoke `nmp_app_podcast_snapshot` — carries everything the UI shows (library, player, downloads, settings), reliable but is a poll. This split means `store_open_failure` is stuck on the push channel that barely fires, and all real data is stuck on a poll that violates D8. [^d0690-20]

## Correct Fix

The fix does not require ADR-0037 or any new NMP capability. The correct fix is: (1) App-side — register the podcast projection via `nmp_app_register_snapshot_projection` so it rides the reactive push frame; delete the bespoke `nmp_app_podcast_snapshot` pull symbol and the 500ms poll. (2) App-side — fix the listener subscribe-timing / rev guard so the first post-Start frame (which carries `store_open_failure`) is actually delivered to `apply()`. (3) NMP-side — verify (and harden with a test if needed) that the first post-Start frame always emits. [^d0690-21]

## Why ADR-0037 Is Not the Answer

The podcast-player agent proposed ADR-0037 as a "keystone" fix — the typed sidecar would push a typed projection through the reactive update callback, killing the poll and carrying `store_open_failure` reactively in one move. This is incorrect: ADR-0037 is a hot-path performance optimization for the Chirp home feed, not a prerequisite for getting onto the push channel. The podcast-player's root error was creating a bespoke pull symbol rather than using the existing `register_snapshot_projection` seam. Reaching for the typed sidecar here confuses an additive performance optimization with the fundamental wiring fix. It also violates NMP's "one way" principle: the registry seam (both generic and typed registration) is the single canonical path; an app does not choose an encoding.

<!-- citations: [^d0690-22] [^d0690-33] -->
## See Also
- [[d8-no-polling-ever|D8 — No Polling, Ever]] — related guide
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[adr-0037-typed-projections-status|ADR-0037 — Typed FlatBuffers Runtime Projections (Proposed, Hot-Path Only)]] — related guide
- [[kernel-boot-initial-emit-guarantee|Kernel Boot Initial Emit — Guaranteed Post-Start Snapshot Frame]] — related guide
- [[builder-guide-projection-docs-gap|Builder-Guide Documentation Gap — register_snapshot_projection Never Taught]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide
- [[builder-guide-projection-docs-gap|Builder-Guide Documentation Gap — register_snapshot_projection Never Taught]] — related guide

