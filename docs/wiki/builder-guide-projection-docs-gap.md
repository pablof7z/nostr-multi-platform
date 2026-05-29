---
title: Builder-Guide Documentation Gap — register_snapshot_projection Never Taught
slug: builder-guide-projection-docs-gap
summary: The 28-chapter builder-guide never mentions register_snapshot_projection — the canonical seam appears only in ADRs and deprecation calendars, causing every downstream app to reinvent the bespoke pull+poll anti-pattern.
tags:
  - docs
  - projection-registry
  - builder-guide
  - anti-pattern
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# Builder-Guide Documentation Gap — register_snapshot_projection Never Taught

> The 28-chapter builder-guide never mentions register_snapshot_projection — the canonical seam appears only in ADRs and deprecation calendars, causing every downstream app to reinvent the bespoke pull+poll anti-pattern.

## The Gap

The 28-chapter builder-guide (`docs/builder-guide/`) never teaches `register_snapshot_projection`. The canonical seam — the single way to publish a projection that rides the reactive push frame — appears only in ADRs (0037/0038/0025, design rationale), the FFI deprecation calendar (what-not-to-use), and escape-hatch documentation. A builder reading the actual 'how to build an app' guide — chapters 15 (FFI), 17 (iOS shell), 19 (microblog walkthrough), 20/21 — is never told 'to add a projection, register it and read it from the pushed snapshot.' [^d0690-40]

## Consequence

Without positive guidance, every downstream app copies the nearest existing example, which is a bespoke `nmp_app_*_snapshot` pull symbol. This is exactly what nmp-gallery did, what Chirp/Marmot did, and what the podcast-player agent did. The docs stress deprecation (negative) and never teach the positive path — the root cause behind repeated ADR-0025 anti-pattern instances. [^d0690-41]

## Required Fix

The builder-guide must include a positive 'How to add a projection' section in the FFI chapter (chapter 15) that demonstrates: registering a projection via `nmp_app_register_snapshot_projection`, reading it from the pushed `SnapshotFrame::projections` map in the shell's `apply()` callback, and the guarantee that registered projections ride every tick's reactive push frame. This section must come before any example that shows bespoke pull symbols. [^d0690-42]

## See Also
- [[adr-0025-bespoke-ffi-anti-pattern|ADR-0025 — Bespoke FFI Pull Symbols Are an Anti-Pattern; Use register_snapshot_projection]] — related guide
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[podcast-player-polling-incident|Podcast-Player Polling Incident — Second-App ADR-0025 Anti-Pattern]] — related guide
- [[one-way-principle|One-Way Principle — Avoid Multiple Mechanisms for the Same Concern]] — related guide

